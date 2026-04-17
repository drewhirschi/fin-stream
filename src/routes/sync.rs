use axum::{
    Router,
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::{get, post},
};
use std::sync::Arc;

use crate::AppState;
use crate::models::*;
use crate::templates;

#[derive(serde::Deserialize)]
struct SyncRunParams {
    slug: Option<String>,
}

#[derive(serde::Deserialize)]
struct SyncCadenceForm {
    sync_cadence: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/sync", get(sync_page))
        .route("/sync/run", post(run_sync))
        .route("/sync/status", get(sync_status))
        .route("/sync/logs", get(sync_logs_partial))
        .route("/integrations/{slug}/sync/run", post(run_integration_sync))
        .route(
            "/integrations/{slug}/sync/status",
            get(integration_sync_status),
        )
        .route(
            "/integrations/{slug}/sync/logs",
            get(integration_sync_logs_partial),
        )
        .route(
            "/integrations/{slug}/sync/cadence",
            post(update_integration_sync_cadence),
        )
        .route(
            "/integrations/tmo/reset-credential",
            post(reset_tmo_credential),
        )
}

async fn sync_page(State(state): State<Arc<AppState>>) -> templates::SyncTemplate {
    let logs: Vec<SyncLog> = sqlx::query_as(
        "SELECT id, connection_slug, started_at, finished_at, status, error_message, endpoints_hit,
                events_upserted, loans_upserted, snapshots_created
         FROM sync_log ORDER BY started_at DESC LIMIT 20",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let status = state.sync_status.lock().await.clone();

    templates::SyncTemplate {
        title: "Trust Deeds - Sync".into(),
        logs,
        current_status: status,
    }
}

/// POST /sync/run — kick off a sync in the background, return immediately.
async fn run_sync(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SyncRunParams>,
) -> impl IntoResponse {
    let slug = params.slug.unwrap_or_else(|| "tmo".to_string());
    start_sync(state, slug, false).await
}

async fn run_integration_sync(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    start_sync(state, slug, true).await
}

async fn start_sync(state: Arc<AppState>, slug: String, scoped: bool) -> impl IntoResponse {
    if slug != "tmo" {
        return axum::response::Html(format!(
            r#"<div class="alert alert-warning">Sync is not wired for {} yet.</div>"#,
            slug
        ));
    }

    // Check if already running
    {
        let status = state.sync_status.lock().await;
        if status
            .as_ref()
            .is_some_and(|s| s.is_running && s.connection_slug == slug)
        {
            return axum::response::Html(
                r#"<div class="alert alert-warning">Sync already in progress</div>"#.to_string(),
            );
        }
    }

    // Set running status
    {
        let mut status = state.sync_status.lock().await;
        *status = Some(SyncStatus {
            connection_slug: slug.clone(),
            phase: "starting".into(),
            started_at: chrono::Utc::now().to_rfc3339(),
            finished_at: None,
            is_running: true,
            error: None,
            loans_synced: 0,
            payments_synced: 0,
        });
    }

    // Spawn the sync task
    let pool = state.db.clone();
    let state_clone = Arc::clone(&state);
    let slug_for_task = slug.clone();

    tokio::spawn(async move {
        let result = match slug_for_task.as_str() {
            "tmo" => crate::tmo::sync::run_full_sync(&pool).await,
            _ => Err(anyhow::anyhow!("sync not wired for {}", slug_for_task)),
        };

        let mut status = state_clone.sync_status.lock().await;
        match result {
            Ok(summary) => {
                *status = Some(SyncStatus {
                    connection_slug: slug_for_task.clone(),
                    phase: "complete".into(),
                    started_at: status
                        .as_ref()
                        .map(|s| s.started_at.clone())
                        .unwrap_or_default(),
                    finished_at: Some(chrono::Utc::now().to_rfc3339()),
                    is_running: false,
                    error: None,
                    loans_synced: summary.loans_upserted,
                    payments_synced: summary.events_upserted,
                });
            }
            Err(e) => {
                tracing::error!("sync failed: {e}");
                *status = Some(SyncStatus {
                    connection_slug: slug_for_task.clone(),
                    phase: "error".into(),
                    started_at: status
                        .as_ref()
                        .map(|s| s.started_at.clone())
                        .unwrap_or_default(),
                    finished_at: Some(chrono::Utc::now().to_rfc3339()),
                    is_running: false,
                    error: Some(e.to_string()),
                    loans_synced: 0,
                    payments_synced: 0,
                });
            }
        }
    });

    let status_url = if scoped {
        format!("/integrations/{slug}/sync/status")
    } else {
        "/sync/status".to_string()
    };

    axum::response::Html(format!(
        r#"<div class="alert alert-info" hx-get="{}" hx-trigger="every 1s" hx-swap="outerHTML">
            Sync started... <span class="loading loading-spinner loading-sm"></span>
        </div>"#,
        status_url
    ))
}

/// GET /sync/status — HTMX polls this to update the sync status indicator.
async fn sync_status(State(state): State<Arc<AppState>>) -> axum::response::Html<String> {
    render_sync_status(state.sync_status.lock().await.as_ref(), None)
}

async fn integration_sync_status(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> axum::response::Html<String> {
    render_sync_status(state.sync_status.lock().await.as_ref(), Some(slug.as_str()))
}

fn render_sync_status(
    status: Option<&SyncStatus>,
    expected_slug: Option<&str>,
) -> axum::response::Html<String> {
    let poll_url = expected_slug
        .map(|slug| format!("/integrations/{slug}/sync/status"))
        .unwrap_or_else(|| "/sync/status".to_string());
    let relevant = status.and_then(|s| {
        if expected_slug
            .map(|slug| slug == s.connection_slug)
            .unwrap_or(true)
        {
            Some(s)
        } else {
            None
        }
    });

    match relevant {
        None => axum::response::Html(
            r#"<div class="alert alert-neutral">No sync has been run yet. Click "Sync now" to start.</div>"#.into(),
        ),
        Some(s) if s.is_running => axum::response::Html(format!(
            r#"<div class="alert alert-info" hx-get="{}" hx-trigger="every 1s" hx-swap="outerHTML">
                <span class="loading loading-spinner loading-sm"></span>
                Syncing: {} ...
            </div>"#,
            poll_url,
            s.phase
        )),
        Some(s) if s.error.is_some() => axum::response::Html(format!(
            r#"<div class="alert alert-error">
                Sync failed: {}
            </div>"#,
            s.error.as_deref().unwrap_or("unknown")
        )),
        Some(s) => axum::response::Html(format!(
            r#"<div class="alert alert-success">
                Sync complete — {} loans, {} events synced
            </div>"#,
            s.loans_synced, s.payments_synced
        )),
    }
}

/// GET /sync/logs — returns the logs table body for HTMX refresh.
async fn sync_logs_partial(
    State(state): State<Arc<AppState>>,
) -> templates::SyncLogsPartialTemplate {
    let logs: Vec<SyncLog> = sqlx::query_as(
        "SELECT id, connection_slug, started_at, finished_at, status, error_message, endpoints_hit,
                events_upserted, loans_upserted, snapshots_created
         FROM sync_log ORDER BY started_at DESC LIMIT 20",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    templates::SyncLogsPartialTemplate { logs }
}

async fn integration_sync_logs_partial(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> templates::SyncLogsPartialTemplate {
    templates::SyncLogsPartialTemplate {
        logs: crate::db::integrations::list_sync_logs_for_connection(&state.db, &slug, 20).await,
    }
}

/// Wipe the encrypted TMO credential so the next sync re-bootstraps from
/// TMO_ACCOUNT / TMO_PIN env vars. Recovery path for the case where
/// APP_ENCRYPTION_KEY was rotated and the stored row is no longer
/// decryptable. Sits behind the protected router (session auth required).
async fn reset_tmo_credential(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let Some(connection) =
        crate::db::integrations::get_connection_by_slug(&state.db, "tmo").await
    else {
        return axum::response::Html(
            r#"<div class="alert alert-warning py-2 text-sm">TMO connection not found.</div>"#
                .to_string(),
        );
    };

    match crate::db::integrations::reset_tmo_credential(&state.db, connection.id).await {
        Ok(rows) => {
            tracing::warn!(
                "operator reset TMO credential ({rows} row deleted); next sync will \
                 re-bootstrap from TMO_ACCOUNT/TMO_PIN"
            );
            axum::response::Html(format!(
                r#"<div class="alert alert-success py-2 text-sm">TMO credential cleared ({rows} row). Run "Sync now" to re-bootstrap.</div>"#
            ))
        }
        Err(err) => {
            tracing::error!("failed to reset TMO credential: {err}");
            axum::response::Html(
                r#"<div class="alert alert-error py-2 text-sm">Could not reset credential — check logs.</div>"#
                    .to_string(),
            )
        }
    }
}

async fn update_integration_sync_cadence(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    axum::extract::Form(form): axum::extract::Form<SyncCadenceForm>,
) -> impl IntoResponse {
    let raw = form.sync_cadence.trim();

    // Reject free-form input — only the typed enum values are allowed. This
    // prevents the "oh, it was `0 21 * * *` not `0 */6 * * *`" mistake that
    // left the TMO integration running once a day.
    let Some(cadence) = crate::scheduler::SyncCadence::parse(raw) else {
        return axum::response::Html(
            r#"<div class="alert alert-error py-2 text-sm">Unknown sync cadence. Pick one of the preset options.</div>"#
                .to_string(),
        );
    };

    if let Err(err) =
        crate::db::integrations::update_sync_cadence(&state.db, &slug, cadence.as_str()).await
    {
        tracing::error!("failed to update sync cadence for {}: {}", slug, err);
        return axum::response::Html(
            r#"<div class="alert alert-error py-2 text-sm">Could not save sync cadence.</div>"#
                .to_string(),
        );
    }

    axum::response::Html(
        r#"<div class="alert alert-success py-2 text-sm">Sync cadence saved.</div>"#.to_string(),
    )
}
