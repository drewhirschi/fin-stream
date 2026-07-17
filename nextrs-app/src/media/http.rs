use std::{fmt::Display, sync::Arc};

use axum::{
    Form, Json,
    extract::{Extension, Path},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use libsql::Connection;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    db::AppContext,
    integrations::{IntegrationRepository, TmoImportLoan},
    templates,
    workspace_inbox::{LoanWorkspaceDraft, WorkspaceInboxError, WorkspaceInboxRepository},
};

use super::{
    MediaError, MediaService, PhotoLocation, UploadIntentDraft, classify_photo,
    key::{canonical_key, encode_route_segment, photo_key_from_route},
};

#[derive(Debug, Default, Deserialize)]
pub struct WorkspaceForm {
    pub redfin_url: Option<String>,
    pub zillow_url: Option<String>,
    pub decision_status: Option<String>,
    pub target_contribution: Option<String>,
    pub actual_contribution: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PhotoUploadIntentRequest {
    pub file_name: String,
    pub content_type: String,
    pub size_bytes: u64,
}

#[derive(Debug, Deserialize)]
pub struct FinalizePhotoUploadRequest {
    pub token: String,
}

#[derive(Debug, Serialize)]
struct FinalizedPhotoResponse {
    photo_id: i64,
    image_url: String,
}

pub async fn save_workspace(
    Extension(context): Extension<AppContext>,
    Path((slug, loan_account)): Path<(String, String)>,
    Form(form): Form<WorkspaceForm>,
) -> Response {
    let (connection, integration_id, _) = match resolve_loan(&context, &slug, &loan_account).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let draft = match workspace_draft(integration_id, &loan_account, form) {
        Ok(draft) => draft,
        Err(()) => return workspace_validation_error(),
    };
    if let Err(error) = WorkspaceInboxRepository::new(&connection)
        .upsert_workspace(&draft)
        .await
    {
        return workspace_repository_error("save loan workspace", error);
    }
    Redirect::to(&workspace_destination(&slug, &loan_account)).into_response()
}

pub async fn create_photo_upload_intent(
    Extension(context): Extension<AppContext>,
    Extension(media): Extension<Arc<MediaService>>,
    Path((slug, loan_account)): Path<(String, String)>,
    Json(request): Json<PhotoUploadIntentRequest>,
) -> Response {
    let (_, integration_id, _) = match resolve_loan(&context, &slug, &loan_account).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    match media.create_upload_intent(UploadIntentDraft {
        connection_id: integration_id,
        loan_account,
        file_name: request.file_name.trim().to_owned(),
        content_type: request.content_type.trim().to_ascii_lowercase(),
        size_bytes: request.size_bytes,
    }) {
        Ok(intent) => Json(intent).into_response(),
        Err(error) => media_json_error(error),
    }
}

pub async fn finalize_photo_upload(
    Extension(context): Extension<AppContext>,
    Extension(media): Extension<Arc<MediaService>>,
    Path((slug, loan_account)): Path<(String, String)>,
    Json(request): Json<FinalizePhotoUploadRequest>,
) -> Response {
    let (connection, integration_id, _) = match resolve_loan(&context, &slug, &loan_account).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    drop(connection);

    // Object verification is deliberately outside a database transaction.
    // A failed DB commit leaves an unreferenced random-key object which can be
    // safely retried with the same short-lived intent or cleaned by lifecycle;
    // it never produces a row that points at missing bytes.
    let verified = match media
        .verify_uploaded_intent(&request.token, integration_id, &loan_account)
        .await
    {
        Ok(verified) => verified,
        Err(error) => return media_json_error(error),
    };

    let (connection, current_integration_id, _) =
        match resolve_loan(&context, &slug, &loan_account).await {
            Ok(value) => value,
            Err(response) => return response,
        };
    if current_integration_id != verified.connection_id {
        return media_json_error(MediaError::InvalidIntent);
    }
    let caption = verified.file_name.trim();
    let photo = match WorkspaceInboxRepository::new(&connection)
        .create_manual_photo_metadata(
            current_integration_id,
            &loan_account,
            (!caption.is_empty()).then_some(caption),
            &verified.image_url,
        )
        .await
    {
        Ok(photo) => photo,
        Err(error) => return workspace_json_error("finalize photo metadata", error),
    };
    Json(FinalizedPhotoResponse {
        photo_id: photo.id,
        image_url: photo.image_url,
    })
    .into_response()
}

pub async fn feature_photo(
    Extension(context): Extension<AppContext>,
    Path((slug, loan_account, photo_id)): Path<(String, String, i64)>,
) -> Response {
    let (connection, integration_id, _) = match resolve_loan(&context, &slug, &loan_account).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    match WorkspaceInboxRepository::new(&connection)
        .set_featured_photo(integration_id, &loan_account, photo_id)
        .await
    {
        Ok(_) => Redirect::to(&workspace_destination(&slug, &loan_account)).into_response(),
        Err(WorkspaceInboxError::NotFound(_)) => templates::not_found_response(),
        Err(error) => workspace_repository_error("feature loan photo", error),
    }
}

/// Remove the durable workspace reference only. Physical object deletion is
/// intentionally deferred: database and S3 cannot share a transaction, and
/// deleting bytes first could leave a committed row pointing at a missing
/// object. Versioning/lifecycle cleanup may reclaim the now-orphaned key after
/// the rollback window and object-manifest reconciliation.
pub async fn delete_photo(
    Extension(context): Extension<AppContext>,
    Path((slug, loan_account, photo_id)): Path<(String, String, i64)>,
) -> Response {
    let (connection, integration_id, _) = match resolve_loan(&context, &slug, &loan_account).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    match WorkspaceInboxRepository::new(&connection)
        .delete_photo_metadata(integration_id, &loan_account, photo_id)
        .await
    {
        Ok(_) => Redirect::to(&workspace_destination(&slug, &loan_account)).into_response(),
        Err(WorkspaceInboxError::NotFound(_)) => templates::not_found_response(),
        Err(error) => workspace_repository_error("remove loan photo metadata", error),
    }
}

pub async fn serve_loan_photo(
    Extension(context): Extension<AppContext>,
    Extension(media): Extension<Arc<MediaService>>,
    Path(route_key): Path<String>,
) -> Response {
    let object_key = match photo_key_from_route(&route_key) {
        Ok(key) => key,
        Err(_) => return templates::not_found_response(),
    };
    let connection = match context.connection().await {
        Ok(connection) => connection,
        Err(error) => return storage_response("open photo authorization database", error),
    };
    let locations = match WorkspaceInboxRepository::new(&connection)
        .photo_image_locations()
        .await
    {
        Ok(locations) => locations,
        Err(error) => return storage_response("authorize loan photo", error),
    };
    let referenced = locations.into_iter().any(|location| {
        matches!(
            classify_photo(&location),
            Ok(PhotoLocation::Stored(candidate)) if candidate == object_key
        )
    });
    if !referenced {
        return templates::not_found_response();
    }
    signed_redirect(&media, &object_key)
}

pub async fn serve_email_object(
    Extension(context): Extension<AppContext>,
    Extension(media): Extension<Arc<MediaService>>,
    Path(route_key): Path<String>,
) -> Response {
    let object_key = match canonical_key(&route_key) {
        Ok(key) => key,
        Err(_) => return templates::not_found_response(),
    };
    let connection = match context.connection().await {
        Ok(connection) => connection,
        Err(error) => return storage_response("open email media authorization database", error),
    };
    match WorkspaceInboxRepository::new(&connection)
        .email_object_is_referenced(&object_key)
        .await
    {
        Ok(true) => signed_redirect(&media, &object_key),
        Ok(false) => templates::not_found_response(),
        Err(error) => storage_response("authorize email media", error),
    }
}

async fn resolve_loan(
    context: &AppContext,
    slug: &str,
    loan_account: &str,
) -> Result<(Connection, i64, TmoImportLoan), Response> {
    if slug.len() > 64 || loan_account.is_empty() || loan_account.len() > 128 {
        return Err(templates::not_found_response());
    }
    let connection = context
        .connection()
        .await
        .map_err(|error| storage_response("open workspace database", error))?;
    let repository = IntegrationRepository::new(&connection);
    let integration = repository
        .connection_by_slug(slug)
        .await
        .map_err(|error| storage_response("load workspace integration", error))?
        .filter(|connection| connection.slug == "tmo" && connection.provider == "mortgage_office")
        .ok_or_else(templates::not_found_response)?;
    let loan = repository
        .tmo_loan_by_account(integration.id, loan_account)
        .await
        .map_err(|error| storage_response("load workspace loan", error))?
        .ok_or_else(templates::not_found_response)?;
    Ok((connection, integration.id, loan))
}

fn workspace_draft(
    connection_id: i64,
    loan_account: &str,
    form: WorkspaceForm,
) -> Result<LoanWorkspaceDraft, ()> {
    Ok(LoanWorkspaceDraft {
        connection_id,
        loan_account: loan_account.to_owned(),
        redfin_url: normalize_optional(form.redfin_url, 2_048)?,
        zillow_url: normalize_optional(form.zillow_url, 2_048)?,
        decision_status: normalize_optional(form.decision_status, 32)?,
        target_contribution: parse_optional_money(form.target_contribution)?,
        actual_contribution: parse_optional_money(form.actual_contribution)?,
        notes: normalize_optional(form.notes, 20_000)?,
    })
}

fn normalize_optional(value: Option<String>, max_bytes: usize) -> Result<Option<String>, ()> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > max_bytes || value.chars().any(|character| character == '\0') {
        return Err(());
    }
    Ok(Some(value.to_owned()))
}

fn parse_optional_money(value: Option<String>) -> Result<Option<f64>, ()> {
    let Some(value) = normalize_optional(value, 64)? else {
        return Ok(None);
    };
    let parsed = value.replace(',', "").parse::<f64>().map_err(|_| ())?;
    if !parsed.is_finite() || parsed < 0.0 {
        return Err(());
    }
    Ok(Some(parsed))
}

fn signed_redirect(media: &MediaService, object_key: &str) -> Response {
    let url = match media.presign_download(object_key) {
        Ok(url) => url,
        Err(MediaError::Disabled) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "Media storage is not configured.",
            )
                .into_response();
        }
        Err(error) => {
            tracing::error!(%error, "could not sign authenticated media redirect");
            return (StatusCode::BAD_GATEWAY, "Media is temporarily unavailable.").into_response();
        }
    };
    let location = match HeaderValue::from_str(&url) {
        Ok(value) => value,
        Err(_) => {
            tracing::error!("object signer returned an invalid redirect URL");
            return (StatusCode::BAD_GATEWAY, "Media is temporarily unavailable.").into_response();
        }
    };
    let mut response = StatusCode::TEMPORARY_REDIRECT.into_response();
    response.headers_mut().insert(header::LOCATION, location);
    response
}

fn workspace_destination(slug: &str, loan_account: &str) -> String {
    format!(
        "/integrations/{}/loans/{}#workspace",
        encode_route_segment(slug),
        encode_route_segment(loan_account)
    )
}

fn workspace_validation_error() -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        "Check the workspace links and contribution amounts, then try again.",
    )
        .into_response()
}

fn media_json_error(error: MediaError) -> Response {
    let (status, code, message) = match error {
        MediaError::InvalidInput | MediaError::InvalidKey | MediaError::InvalidIntent => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_media_request",
            "The media request is invalid.",
        ),
        MediaError::ExpiredIntent => (
            StatusCode::CONFLICT,
            "upload_expired",
            "The upload expired. Choose the file again.",
        ),
        MediaError::ObjectMissing => (
            StatusCode::CONFLICT,
            "upload_missing",
            "The direct upload has not completed.",
        ),
        MediaError::ObjectMismatch => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "upload_mismatch",
            "The uploaded object does not match the requested file.",
        ),
        MediaError::Disabled | MediaError::Configuration => (
            StatusCode::SERVICE_UNAVAILABLE,
            "media_unavailable",
            "Media storage is not configured.",
        ),
        MediaError::StorageUnavailable => (
            StatusCode::BAD_GATEWAY,
            "media_unavailable",
            "Media storage is temporarily unavailable.",
        ),
    };
    (status, Json(json!({ "error": code, "message": message }))).into_response()
}

fn workspace_repository_error(operation: &'static str, error: WorkspaceInboxError) -> Response {
    match error {
        WorkspaceInboxError::Validation(_) => workspace_validation_error(),
        WorkspaceInboxError::NotFound(_) => templates::not_found_response(),
        error => storage_response(operation, error),
    }
}

fn workspace_json_error(operation: &'static str, error: WorkspaceInboxError) -> Response {
    match error {
        WorkspaceInboxError::Validation(_) => media_json_error(MediaError::InvalidInput),
        WorkspaceInboxError::NotFound(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "not_found", "message": "Photo not found." })),
        )
            .into_response(),
        WorkspaceInboxError::Conflict(_) => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "photo_conflict",
                "message": "The photo changed. Refresh and try again."
            })),
        )
            .into_response(),
        error => {
            tracing::error!(%error, operation, "workspace media database failure");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "storage_unavailable",
                    "message": "Could not save the photo."
                })),
            )
                .into_response()
        }
    }
}

fn storage_response(operation: &'static str, error: impl Display) -> Response {
    tracing::error!(%error, operation, "workspace media storage failure");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Could not complete this request.",
    )
        .into_response()
}
