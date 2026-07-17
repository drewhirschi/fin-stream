use std::{collections::BTreeMap, sync::Arc, time::Duration};

use axum::{
    Form, Json,
    body::{Body, Bytes, to_bytes},
    extract::{Extension, Path, Request},
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;

use crate::{
    db::AppContext,
    media::{MediaError, MediaService},
    workspace_inbox::{ReceivedEmailAttachment, WorkspaceInboxError},
};

use super::{
    ClaimOutcome, ClaimedEmail, InboundAttachmentDraft, InboundEmailDraft, InboundEmailRepository,
    ResendService,
    client::{ProviderError, ResendProvider},
};

const WEBHOOK_PATH: &str = "/webhooks/resend";
const MAX_WEBHOOK_BYTES: usize = 256 * 1_024;
const PROCESSING_TIMEOUT: Duration = Duration::from_secs(45);
const RETRY_AFTER_SECONDS: &str = "60";

#[derive(Clone, Copy, Debug)]
pub(crate) struct VerifiedResendWebhook;

pub(crate) fn is_webhook_path(path: &str) -> bool {
    path == WEBHOOK_PATH
}

/// Authenticate the exact raw request body before sessions, browser auth, or
/// the durable write gate can touch libSQL. The verified marker is consumed by
/// the browser-auth middleware so this third-party route never becomes a
/// blanket public mutation exemption.
pub(crate) async fn authenticate_webhook(
    Extension(service): Extension<Arc<ResendService>>,
    mut request: Request,
    next: Next,
) -> Response {
    if !is_webhook_path(request.uri().path()) {
        return next.run(request).await;
    }
    let Some(verifier) = service.verifier() else {
        return webhook_error(StatusCode::SERVICE_UNAVAILABLE, "webhook_unavailable", true);
    };
    let body = match to_bytes(std::mem::take(request.body_mut()), MAX_WEBHOOK_BYTES).await {
        Ok(body) => body,
        Err(_) => {
            return webhook_error(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", false);
        }
    };
    if verifier
        .verify(request.headers(), &body, OffsetDateTime::now_utc())
        .is_err()
    {
        return webhook_error(StatusCode::UNAUTHORIZED, "invalid_signature", false);
    }
    *request.body_mut() = Body::from(body);
    request.extensions_mut().insert(VerifiedResendWebhook);
    next.run(request).await
}

pub async fn webhook(
    Extension(context): Extension<AppContext>,
    Extension(service): Extension<Arc<ResendService>>,
    Extension(media): Extension<Arc<MediaService>>,
    body: Bytes,
) -> Response {
    let raw = match std::str::from_utf8(&body) {
        Ok(raw) => raw,
        Err(_) => return webhook_error(StatusCode::BAD_REQUEST, "invalid_json", false),
    };
    let event: WebhookEvent = match serde_json::from_slice(&body) {
        Ok(event) => event,
        Err(_) => return webhook_error(StatusCode::BAD_REQUEST, "invalid_json", false),
    };
    if event.event_type != "email.received" {
        return Json(json!({ "status": "ignored" })).into_response();
    }
    let draft = match inbound_draft(event, raw) {
        Ok(draft) => draft,
        Err(()) => return webhook_error(StatusCode::BAD_REQUEST, "invalid_event", false),
    };
    let Some(provider) = service.provider() else {
        return webhook_error(StatusCode::SERVICE_UNAVAILABLE, "webhook_unavailable", true);
    };
    let connection = match context.connection().await {
        Ok(connection) => connection,
        Err(_) => return webhook_error(StatusCode::SERVICE_UNAVAILABLE, "database_error", true),
    };
    let repository = InboundEmailRepository::new(&connection);
    let outcome = match repository.claim_webhook(&draft, &new_lease_token()).await {
        Ok(outcome) => outcome,
        Err(WorkspaceInboxError::Validation(_)) | Err(WorkspaceInboxError::Conflict(_)) => {
            return webhook_error(StatusCode::BAD_REQUEST, "invalid_event", false);
        }
        Err(_) => return webhook_error(StatusCode::SERVICE_UNAVAILABLE, "database_error", true),
    };
    run_outcome(&repository, provider.as_ref(), media.as_ref(), outcome).await
}

#[derive(Debug, Default, Deserialize)]
pub struct RetryForm {
    #[serde(default)]
    return_to: Option<String>,
}

pub async fn retry(
    Extension(context): Extension<AppContext>,
    Extension(service): Extension<Arc<ResendService>>,
    Extension(media): Extension<Arc<MediaService>>,
    Path(email_id): Path<i64>,
    Form(form): Form<RetryForm>,
) -> Response {
    if email_id <= 0 {
        return (StatusCode::NOT_FOUND, "Email not found.").into_response();
    }
    let Some(provider) = service.provider() else {
        return retry_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Inbound email processing is not configured.",
            true,
        );
    };
    let connection = match context.connection().await {
        Ok(connection) => connection,
        Err(_) => {
            return retry_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Email storage is temporarily unavailable.",
                true,
            );
        }
    };
    let repository = InboundEmailRepository::new(&connection);
    let outcome = match repository.claim_retry(email_id, &new_lease_token()).await {
        Ok(outcome) => outcome,
        Err(WorkspaceInboxError::NotFound(_)) => {
            return (StatusCode::NOT_FOUND, "Email not found.").into_response();
        }
        Err(_) => {
            return retry_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Email storage is temporarily unavailable.",
                true,
            );
        }
    };
    match outcome {
        ClaimOutcome::AlreadyStored(_) => retry_redirect(email_id, form.return_to.as_deref()),
        ClaimOutcome::Busy(_) => retry_error(
            StatusCode::CONFLICT,
            "This email is already being processed. Try again shortly.",
            true,
        ),
        ClaimOutcome::Acquired(claim) => {
            match run_claim(&repository, provider.as_ref(), media.as_ref(), claim).await {
                Ok(()) => retry_redirect(email_id, form.return_to.as_deref()),
                Err(failure) => retry_error(
                    failure.status(),
                    "The email could not be stored. You can retry it safely.",
                    failure.retryable(),
                ),
            }
        }
    }
}

async fn run_outcome(
    repository: &InboundEmailRepository<'_>,
    provider: &dyn ResendProvider,
    media: &MediaService,
    outcome: ClaimOutcome,
) -> Response {
    match outcome {
        ClaimOutcome::AlreadyStored(_) => {
            Json(json!({ "status": "already_stored" })).into_response()
        }
        ClaimOutcome::Busy(_) => webhook_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "processing_in_progress",
            true,
        ),
        ClaimOutcome::Acquired(claim) => {
            match run_claim(repository, provider, media, claim).await {
                Ok(()) => Json(json!({ "status": "stored" })).into_response(),
                Err(failure) => {
                    webhook_error(failure.status(), failure.code(), failure.retryable())
                }
            }
        }
    }
}

async fn run_claim(
    repository: &InboundEmailRepository<'_>,
    provider: &dyn ResendProvider,
    media: &MediaService,
    claim: ClaimedEmail,
) -> Result<(), WorkflowFailure> {
    let email_id = claim.email.id;
    let lease_token = claim.lease_token.clone();
    let result = tokio::time::timeout(
        PROCESSING_TIMEOUT,
        process_claim(repository, provider, media, &claim),
    )
    .await
    .unwrap_or(Err(WorkflowFailure::Transient));
    if let Err(failure) = result {
        let public_error = if failure.retryable() {
            "Email provider or object storage is temporarily unavailable. Retry is available."
        } else {
            "Email content could not be stored because the provider response was invalid."
        };
        let _ = repository.fail(email_id, &lease_token, public_error).await;
        tracing::warn!(
            email_id,
            failure = failure.code(),
            "inbound email processing failed"
        );
        return Err(failure);
    }
    Ok(())
}

async fn process_claim(
    repository: &InboundEmailRepository<'_>,
    provider: &dyn ResendProvider,
    media: &MediaService,
    claim: &ClaimedEmail,
) -> Result<(), WorkflowFailure> {
    if claim.email.body_content_type.is_none() {
        let content = provider
            .get_received_email(&claim.email.resend_email_id)
            .await
            .map_err(WorkflowFailure::provider)?;
        let body = content
            .html
            .filter(|value| !value.is_empty())
            .map(|value| (value.into_bytes(), "html", "text/html"))
            .or_else(|| {
                content
                    .text
                    .filter(|value| !value.is_empty())
                    .map(|value| (value.into_bytes(), "txt", "text/plain"))
            });
        if let Some((bytes, extension, content_type)) = body {
            let key = format!("emails/{}/body.{extension}", claim.email.resend_email_id);
            media
                .put_canonical_if_absent(&key, bytes, content_type)
                .await
                .map_err(WorkflowFailure::media)?;
            repository
                .record_body(claim.email.id, &claim.lease_token, Some(&key), content_type)
                .await
                .map_err(|_| WorkflowFailure::Transient)?;
        } else {
            repository
                .record_body(claim.email.id, &claim.lease_token, None, "text/plain")
                .await
                .map_err(|_| WorkflowFailure::Transient)?;
        }
    }

    let pending = claim
        .attachments
        .iter()
        .filter(|attachment| attachment.processing_state != "stored")
        .collect::<Vec<_>>();
    if !pending.is_empty() {
        let remote = provider
            .list_attachments(&claim.email.resend_email_id)
            .await
            .map_err(WorkflowFailure::provider)?;
        let mut by_id = BTreeMap::new();
        for attachment in remote {
            if by_id.insert(attachment.id.clone(), attachment).is_some() {
                return Err(WorkflowFailure::Permanent);
            }
        }
        for attachment in pending {
            if let Err(failure) =
                store_attachment(repository, provider, media, claim, attachment, &by_id).await
            {
                let _ = repository
                    .mark_attachment_error(claim.email.id, attachment.id, &claim.lease_token)
                    .await;
                return Err(failure);
            }
        }
    }
    repository
        .complete(claim.email.id, &claim.lease_token)
        .await
        .map_err(|_| WorkflowFailure::Transient)?;
    Ok(())
}

async fn store_attachment(
    repository: &InboundEmailRepository<'_>,
    provider: &dyn ResendProvider,
    media: &MediaService,
    claim: &ClaimedEmail,
    attachment: &ReceivedEmailAttachment,
    remote: &BTreeMap<String, super::ProviderAttachment>,
) -> Result<(), WorkflowFailure> {
    let metadata = remote
        .get(&attachment.resend_attachment_id)
        .ok_or(WorkflowFailure::Permanent)?;
    if metadata.filename != attachment.filename || metadata.content_type != attachment.content_type
    {
        return Err(WorkflowFailure::Permanent);
    }
    let download_url = metadata
        .download_url
        .as_deref()
        .ok_or(WorkflowFailure::Permanent)?;
    let download = provider
        .download_attachment(download_url)
        .await
        .map_err(WorkflowFailure::provider)?;
    if download.content_type != attachment.content_type
        && download.content_type != "application/octet-stream"
    {
        return Err(WorkflowFailure::Permanent);
    }
    let key = format!(
        "emails/{}/attachments/{}",
        claim.email.resend_email_id, attachment.resend_attachment_id
    );
    let size_bytes = i64::try_from(download.bytes.len()).map_err(|_| WorkflowFailure::Permanent)?;
    media
        .put_canonical_if_absent(&key, download.bytes, &attachment.content_type)
        .await
        .map_err(WorkflowFailure::media)?;
    repository
        .mark_attachment_stored(
            claim.email.id,
            attachment.id,
            &claim.lease_token,
            &key,
            size_bytes,
        )
        .await
        .map_err(|_| WorkflowFailure::Transient)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkflowFailure {
    Transient,
    Permanent,
}

impl WorkflowFailure {
    const fn provider(error: ProviderError) -> Self {
        if error.is_transient() {
            Self::Transient
        } else {
            Self::Permanent
        }
    }

    const fn media(error: MediaError) -> Self {
        match error {
            MediaError::InvalidInput | MediaError::InvalidKey | MediaError::ObjectMismatch => {
                Self::Permanent
            }
            MediaError::Configuration
            | MediaError::Disabled
            | MediaError::InvalidIntent
            | MediaError::ExpiredIntent
            | MediaError::ObjectMissing
            | MediaError::StorageUnavailable => Self::Transient,
        }
    }

    const fn retryable(self) -> bool {
        matches!(self, Self::Transient)
    }

    const fn status(self) -> StatusCode {
        match self {
            Self::Transient => StatusCode::SERVICE_UNAVAILABLE,
            Self::Permanent => StatusCode::BAD_GATEWAY,
        }
    }

    const fn code(self) -> &'static str {
        match self {
            Self::Transient => "processing_unavailable",
            Self::Permanent => "invalid_provider_response",
        }
    }
}

#[derive(Deserialize)]
struct WebhookEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    data: serde_json::Value,
}

#[derive(Deserialize)]
struct ReceivedEmailData {
    email_id: String,
    from: String,
    to: Vec<String>,
    subject: Option<String>,
    #[serde(default)]
    attachments: Vec<AttachmentData>,
}

#[derive(Deserialize)]
struct AttachmentData {
    id: String,
    filename: String,
    content_type: String,
}

fn inbound_draft(event: WebhookEvent, raw: &str) -> Result<InboundEmailDraft, ()> {
    let received_at = event.created_at.ok_or(())?;
    chrono::DateTime::parse_from_rfc3339(&received_at).map_err(|_| ())?;
    let data: ReceivedEmailData = serde_json::from_value(event.data).map_err(|_| ())?;
    let to_addresses = serde_json::to_string(&data.to).map_err(|_| ())?;
    Ok(InboundEmailDraft {
        resend_email_id: data.email_id,
        from_address: data.from,
        to_addresses,
        subject: data.subject,
        received_at,
        raw_webhook_payload: raw.to_owned(),
        attachments: data
            .attachments
            .into_iter()
            .map(|attachment| InboundAttachmentDraft {
                resend_attachment_id: attachment.id,
                filename: attachment.filename,
                content_type: attachment.content_type.trim().to_ascii_lowercase(),
            })
            .collect(),
    })
}

fn new_lease_token() -> String {
    let mut value = [0_u8; 32];
    OsRng.fill_bytes(&mut value);
    URL_SAFE_NO_PAD.encode(value)
}

fn webhook_error(status: StatusCode, code: &str, retryable: bool) -> Response {
    let mut response = (status, Json(json!({ "error": code }))).into_response();
    if retryable {
        response.headers_mut().insert(
            header::RETRY_AFTER,
            HeaderValue::from_static(RETRY_AFTER_SECONDS),
        );
    }
    response
}

fn retry_error(status: StatusCode, message: &'static str, retryable: bool) -> Response {
    let mut response = (status, message).into_response();
    if retryable {
        response.headers_mut().insert(
            header::RETRY_AFTER,
            HeaderValue::from_static(RETRY_AFTER_SECONDS),
        );
    }
    response
}

fn retry_redirect(email_id: i64, return_to: Option<&str>) -> Response {
    let destination = if return_to == Some("inbox") {
        "/inbox".to_owned()
    } else {
        format!("/inbox/{email_id}")
    };
    Redirect::to(&destination).into_response()
}
