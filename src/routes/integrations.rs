//! Integration-layer HTTP handlers. Everything here is allowed to reach into
//! `intg.*` tables and TMO-shaped types. The boundary check (see
//! `tools/check_intg_boundary.sh`) allowlists this file; nothing else in
//! `src/routes/**` should reference `intg.*` or TMO-shaped columns.

use axum::{
    Router,
    extract::{DefaultBodyLimit, Form, Multipart, Path, Query, State},
    response::{IntoResponse, Redirect},
    routing::{get, post},
};
use rand::{Rng, distributions::Alphanumeric};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;
use crate::db;
use crate::templates;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/loans", get(legacy_loans))
        .route("/payments", get(legacy_payments))
        .route("/integrations", get(integrations))
        .route("/integrations/{slug}", get(integration_overview))
        .route("/integrations/{slug}/loans", get(integration_loans))
        .route(
            "/integrations/{slug}/loans/{loan_account}",
            get(integration_loan_detail),
        )
        .route(
            "/integrations/{slug}/loans/{loan_account}/workspace",
            post(save_loan_workspace),
        )
        .route(
            "/integrations/{slug}/loans/{loan_account}/workspace/photos",
            post(upload_loan_workspace_photos).layer(DefaultBodyLimit::max(25 * 1024 * 1024)),
        )
        .route(
            "/integrations/{slug}/loans/{loan_account}/workspace/photos/{photo_id}/feature",
            post(set_featured_loan_workspace_photo),
        )
        .route("/integrations/{slug}/payments", get(integration_payments))
        .route("/integrations/{slug}/sync", get(integration_sync))
        .route("/integrations/{slug}/debug", get(integration_debug))
}

#[derive(Deserialize, Default)]
struct LoanWorkspaceParams {
    workspace_saved: Option<i32>,
    workspace_error: Option<i32>,
    photo_uploaded: Option<i32>,
    photo_error: Option<i32>,
    feature_saved: Option<i32>,
}

#[derive(Deserialize, Default)]
struct LoanWorkspaceForm {
    redfin_url: Option<String>,
    zillow_url: Option<String>,
    decision_status: Option<String>,
    target_contribution: Option<String>,
    actual_contribution: Option<String>,
    notes: Option<String>,
}

async fn legacy_loans() -> Redirect {
    Redirect::permanent("/integrations/tmo/loans")
}

async fn legacy_payments() -> Redirect {
    Redirect::permanent("/integrations/tmo/payments")
}

async fn integrations(State(state): State<Arc<AppState>>) -> templates::IntegrationsTemplate {
    templates::IntegrationsTemplate {
        title: "Trust Deeds - Integrations".into(),
        connections: db::integrations::list_connections(&state.db).await,
    }
}

async fn integration_overview(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> axum::response::Response {
    let Some(connection) = connection_or_404(&state, &slug).await else {
        return not_found_for_integration(&slug).into_response();
    };

    let is_tmo = connection.slug == "tmo";
    let (loans, payments, snapshot) = if is_tmo {
        let (loans, payments, snapshot) = tokio::join!(
            db::integrations::list_active_tmo_loans(&state.db),
            db::events::get_recent_payments(&state.db, 8),
            async {
                sqlx::query_as::<_, (Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<f64>)>(
                    "SELECT portfolio_value, portfolio_yield, ytd_interest, trust_balance, outstanding_checks
                     FROM portfolio_snapshot ORDER BY snapshot_date DESC LIMIT 1",
                )
                .fetch_optional(&state.db)
                .await
                .unwrap_or(None)
            }
        );
        (loans, payments, snapshot)
    } else {
        (Vec::new(), Vec::new(), None)
    };

    let (portfolio_value, portfolio_yield, ytd_interest, trust_balance, outstanding_checks) =
        snapshot.unwrap_or((None, None, None, None, None));

    let active_loans_count = loans.len() as i64;

    templates::IntegrationOverviewTemplate {
        title: format!("Trust Deeds - {}", connection.name),
        current_section: "overview".into(),
        loans,
        payments,
        portfolio_value,
        portfolio_yield,
        ytd_interest,
        trust_balance,
        outstanding_checks,
        active_loans_count,
        connection,
    }
    .into_response()
}

async fn integration_loans(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> axum::response::Response {
    let Some(connection) = connection_or_404(&state, &slug).await else {
        return not_found_for_integration(&slug).into_response();
    };

    templates::IntegrationLoansTemplate {
        title: format!("Trust Deeds - {} Loans", connection.name),
        current_section: "loans".into(),
        loans: if connection.slug == "tmo" {
            db::integrations::list_active_tmo_loans(&state.db).await
        } else {
            Vec::new()
        },
        connection,
    }
    .into_response()
}

async fn integration_payments(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> axum::response::Response {
    let Some(connection) = connection_or_404(&state, &slug).await else {
        return not_found_for_integration(&slug).into_response();
    };

    templates::IntegrationPaymentsTemplate {
        title: format!("Trust Deeds - {} Payments", connection.name),
        current_section: "payments".into(),
        payments: if connection.slug == "tmo" {
            db::integrations::list_recent_tmo_import_payments(&state.db, connection.id, 100).await
        } else {
            Vec::new()
        },
        connection,
    }
    .into_response()
}

async fn integration_loan_detail(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LoanWorkspaceParams>,
    Path((slug, loan_account)): Path<(String, String)>,
) -> axum::response::Response {
    let Some(connection) = connection_or_404(&state, &slug).await else {
        return not_found_for_integration(&slug).into_response();
    };

    let Some(loan) = (if connection.slug == "tmo" {
        db::integrations::get_tmo_loan_by_account(&state.db, &loan_account).await
    } else {
        None
    }) else {
        return templates::NotFoundTemplate {
            title: "Trust Deeds - Not Found".into(),
            path: format!("/integrations/{slug}/loans/{loan_account}"),
        }
        .into_response();
    };

    let payment_history = if connection.slug == "tmo" {
        db::integrations::list_tmo_import_payments_for_loan(
            &state.db,
            connection.id,
            &loan_account,
            36,
        )
        .await
    } else {
        Vec::new()
    };
    let workspace = db::workspaces::get_loan_workspace(&state.db, connection.id, &loan_account)
        .await
        .unwrap_or_else(|| crate::models::LoanWorkspaceView::empty(loan_account.clone()));
    let workspace_photos =
        db::workspaces::list_loan_workspace_photos(&state.db, connection.id, &loan_account)
            .await
            .unwrap_or_default();
    let loan_emails = db::emails::list_emails_for_loan(&state.db, &loan_account).await;

    templates::IntegrationLoanDetailTemplate {
        title: format!("Trust Deeds - {} {}", connection.name, loan.loan_account),
        current_section: "loans".into(),
        loan,
        workspace,
        workspace_photos,
        payment_history,
        loan_emails,
        workspace_saved: params.workspace_saved == Some(1),
        workspace_error: params.workspace_error == Some(1),
        photo_uploaded: params.photo_uploaded == Some(1),
        photo_error: params.photo_error == Some(1),
        feature_saved: params.feature_saved == Some(1),
        connection,
    }
    .into_response()
}

async fn save_loan_workspace(
    State(state): State<Arc<AppState>>,
    Path((slug, loan_account)): Path<(String, String)>,
    Form(form): Form<LoanWorkspaceForm>,
) -> axum::response::Response {
    let destination = format!("/integrations/{slug}/loans/{loan_account}");

    let Some(connection) = connection_or_404(&state, &slug).await else {
        return not_found_for_integration(&slug).into_response();
    };

    let loan_exists = if connection.slug == "tmo" {
        db::integrations::get_tmo_loan_by_account(&state.db, &loan_account)
            .await
            .is_some()
    } else {
        false
    };

    if !loan_exists {
        return templates::NotFoundTemplate {
            title: "Trust Deeds - Not Found".into(),
            path: destination,
        }
        .into_response();
    }

    let redfin_url = match normalize_optional_url(form.redfin_url) {
        Ok(value) => value,
        Err(_) => return Redirect::to(&format!("{destination}?workspace_error=1")).into_response(),
    };
    let zillow_url = match normalize_optional_url(form.zillow_url) {
        Ok(value) => value,
        Err(_) => return Redirect::to(&format!("{destination}?workspace_error=1")).into_response(),
    };
    let decision_status = normalize_optional_text(form.decision_status);
    let target_contribution = match parse_optional_currency(form.target_contribution) {
        Ok(value) => value,
        Err(_) => return Redirect::to(&format!("{destination}?workspace_error=1")).into_response(),
    };
    let actual_contribution = match parse_optional_currency(form.actual_contribution) {
        Ok(value) => value,
        Err(_) => return Redirect::to(&format!("{destination}?workspace_error=1")).into_response(),
    };
    let notes = normalize_optional_text(form.notes);

    if let Err(error) = db::workspaces::upsert_loan_workspace(
        &state.db,
        connection.id,
        &loan_account,
        redfin_url.as_deref(),
        zillow_url.as_deref(),
        decision_status.as_deref(),
        target_contribution,
        actual_contribution,
        notes.as_deref(),
    )
    .await
    {
        tracing::error!(
            "failed to save loan workspace for {}: {}",
            loan_account,
            error
        );
        return Redirect::to(&format!("{destination}?workspace_error=1")).into_response();
    }

    Redirect::to(&format!("{destination}?workspace_saved=1")).into_response()
}

async fn upload_loan_workspace_photos(
    State(state): State<Arc<AppState>>,
    Path((slug, loan_account)): Path<(String, String)>,
    mut multipart: Multipart,
) -> axum::response::Response {
    let destination = format!("/integrations/{slug}/loans/{loan_account}");

    let Some(connection) = connection_or_404(&state, &slug).await else {
        return not_found_for_integration(&slug).into_response();
    };

    let loan_exists = if connection.slug == "tmo" {
        db::integrations::get_tmo_loan_by_account(&state.db, &loan_account)
            .await
            .is_some()
    } else {
        false
    };

    if !loan_exists {
        return templates::NotFoundTemplate {
            title: "Trust Deeds - Not Found".into(),
            path: destination,
        }
        .into_response();
    }

    let storage = match crate::media_storage::MediaStorage::from_env().await {
        Ok(storage) => storage,
        Err(error) => {
            tracing::error!("failed to initialize media storage: {}", error);
            return Redirect::to(&format!("{destination}?photo_error=1")).into_response();
        }
    };

    let mut sort_order = match db::workspaces::next_photo_sort_order(
        &state.db,
        connection.id,
        &loan_account,
    )
    .await
    {
        Ok(sort_order) => sort_order,
        Err(error) => {
            tracing::error!("failed to read next photo sort order: {}", error);
            return Redirect::to(&format!("{destination}?photo_error=1")).into_response();
        }
    };
    let source_url = db::workspaces::get_loan_workspace(&state.db, connection.id, &loan_account)
        .await
        .and_then(|workspace| workspace.zillow_url.or(workspace.redfin_url))
        .unwrap_or_else(|| "manual-upload".into());

    let mut uploaded_any = false;
    loop {
        let next_field = match multipart.next_field().await {
            Ok(next) => next,
            Err(error) => {
                tracing::error!("failed reading multipart field: {}", error);
                return Redirect::to(&format!("{destination}?photo_error=1")).into_response();
            }
        };

        let Some(field) = next_field else {
            break;
        };

        if field.name() != Some("photos") {
            continue;
        }

        let content_type = field.content_type().map(ToString::to_string);
        if !content_type
            .as_deref()
            .map(is_supported_image_content_type)
            .unwrap_or(false)
        {
            continue;
        }

        let file_name = field.file_name().map(ToString::to_string);
        let bytes = match field.bytes().await {
            Ok(bytes) if !bytes.is_empty() => bytes.to_vec(),
            Ok(_) => continue,
            Err(error) => {
                tracing::error!("failed reading uploaded photo bytes: {}", error);
                return Redirect::to(&format!("{destination}?photo_error=1")).into_response();
            }
        };

        let extension = file_name
            .as_deref()
            .and_then(file_extension_from_name)
            .or_else(|| {
                content_type
                    .as_deref()
                    .and_then(content_type_to_extension)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "jpg".into());
        let object_key = format!(
            "{}/manual-{}-{}.{}",
            sanitize_path_segment(&loan_account),
            sort_order + 1,
            random_suffix(),
            extension
        );

        let stored = match storage
            .store(&object_key, bytes, content_type.as_deref())
            .await
        {
            Ok(stored) => stored,
            Err(error) => {
                tracing::error!("failed storing uploaded photo {}: {}", object_key, error);
                return Redirect::to(&format!("{destination}?photo_error=1")).into_response();
            }
        };

        let caption = file_name
            .as_deref()
            .and_then(non_empty_trimmed)
            .unwrap_or("Manual upload");
        if let Err(error) = db::workspaces::insert_loan_workspace_photo(
            &state.db,
            connection.id,
            &loan_account,
            "manual",
            Some(caption),
            &source_url,
            &stored.public_url,
            sort_order,
        )
        .await
        {
            tracing::error!("failed saving uploaded photo row: {}", error);
            return Redirect::to(&format!("{destination}?photo_error=1")).into_response();
        }

        uploaded_any = true;
        sort_order += 1;
    }

    if !uploaded_any {
        return Redirect::to(&format!("{destination}?photo_error=1")).into_response();
    }

    Redirect::to(&format!("{destination}?photo_uploaded=1")).into_response()
}

async fn set_featured_loan_workspace_photo(
    State(state): State<Arc<AppState>>,
    Path((slug, loan_account, photo_id)): Path<(String, String, i64)>,
) -> axum::response::Response {
    let destination = format!("/integrations/{slug}/loans/{loan_account}");

    let Some(connection) = connection_or_404(&state, &slug).await else {
        return not_found_for_integration(&slug).into_response();
    };

    if let Err(error) =
        db::workspaces::set_featured_photo(&state.db, connection.id, &loan_account, photo_id).await
    {
        tracing::error!(
            "failed to mark featured photo for loan {} photo {}: {}",
            loan_account,
            photo_id,
            error
        );
        return Redirect::to(&format!("{destination}?photo_error=1")).into_response();
    }

    Redirect::to(&format!("{destination}?feature_saved=1")).into_response()
}

async fn integration_sync(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> axum::response::Response {
    let Some(connection) = connection_or_404(&state, &slug).await else {
        return not_found_for_integration(&slug).into_response();
    };

    templates::IntegrationSyncTemplate {
        title: format!("Trust Deeds - {} Sync", connection.name),
        current_section: "sync".into(),
        sync_logs: db::integrations::list_sync_logs_for_connection(&state.db, &connection.slug, 20)
            .await,
        connection,
    }
    .into_response()
}

async fn integration_debug(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> axum::response::Response {
    let Some(connection) = connection_or_404(&state, &slug).await else {
        return not_found_for_integration(&slug).into_response();
    };

    templates::IntegrationDebugTemplate {
        title: format!("Trust Deeds - {} Debug", connection.name),
        current_section: "debug".into(),
        sync_logs: db::integrations::list_sync_logs_for_connection(&state.db, &connection.slug, 20)
            .await,
        tmo_import_payments: if connection.slug == "tmo" {
            db::integrations::list_recent_tmo_import_payments(&state.db, connection.id, 30).await
        } else {
            Vec::new()
        },
        captured_records: db::integrations::list_captured_records_for_connection(
            &state.db,
            connection.id,
            20,
        )
        .await,
        normalized_payments: if connection.slug == "tmo" {
            db::integrations::list_normalized_payments(&state.db, 20).await
        } else {
            Vec::new()
        },
        connection,
    }
    .into_response()
}

async fn connection_or_404(
    state: &Arc<AppState>,
    slug: &str,
) -> Option<crate::models::IntegrationConnectionView> {
    db::integrations::get_connection_by_slug(&state.db, slug).await
}

fn not_found_for_integration(slug: &str) -> templates::NotFoundTemplate {
    templates::NotFoundTemplate {
        title: "Trust Deeds - Not Found".into(),
        path: format!("/integrations/{slug}"),
    }
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn normalize_optional_url(value: Option<String>) -> Result<Option<String>, ()> {
    let Some(value) = normalize_optional_text(value) else {
        return Ok(None);
    };

    if value.starts_with("http://") || value.starts_with("https://") {
        Ok(Some(value))
    } else {
        Err(())
    }
}

fn parse_optional_currency(value: Option<String>) -> Result<Option<f64>, ()> {
    let Some(value) = normalize_optional_text(value) else {
        return Ok(None);
    };

    let normalized = value.replace(',', "");
    let parsed: f64 = normalized.parse().map_err(|_| ())?;
    if parsed < 0.0 {
        return Err(());
    }

    Ok(Some(parsed))
}

fn is_supported_image_content_type(content_type: &str) -> bool {
    matches!(
        content_type.split(';').next().unwrap_or_default().trim(),
        "image/jpeg" | "image/png" | "image/webp"
    )
}

fn file_extension_from_name(file_name: &str) -> Option<String> {
    let extension = file_name.rsplit('.').next()?.trim().to_ascii_lowercase();
    match extension.as_str() {
        "jpg" | "jpeg" | "png" | "webp" => Some(extension),
        _ => None,
    }
}

fn content_type_to_extension(content_type: &str) -> Option<&'static str> {
    match content_type.split(';').next()?.trim() {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

fn sanitize_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn random_suffix() -> String {
    rand::thread_rng()
        .sample_iter(Alphanumeric)
        .take(8)
        .map(char::from)
        .collect()
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}
