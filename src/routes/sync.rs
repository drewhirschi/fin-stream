use axum::{Router, extract::State, routing::{get, post}, response::IntoResponse};
use std::sync::Arc;

use crate::AppState;
use crate::models::*;
use crate::templates;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/sync", get(sync_page))
        .route("/sync/run", post(run_sync))
        .route("/sync/status", get(sync_status))
        .route("/sync/logs", get(sync_logs_partial))
}

async fn sync_page(State(state): State<Arc<AppState>>) -> templates::SyncTemplate {
    let logs: Vec<SyncLog> = sqlx::query_as(
        "SELECT id, started_at, finished_at, status, error_message, endpoints_hit,
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
async fn run_sync(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Check if already running
    {
        let status = state.sync_status.lock().await;
        if status.as_ref().is_some_and(|s| s.is_running) {
            return axum::response::Html(
                r#"<div class="alert alert-warning">Sync already in progress</div>"#.to_string(),
            );
        }
    }

    // Set running status
    {
        let mut status = state.sync_status.lock().await;
        *status = Some(SyncStatus {
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

    tokio::spawn(async move {
        let result = crate::tmo::sync::run_full_sync(&pool).await;

        let mut status = state_clone.sync_status.lock().await;
        match result {
            Ok(summary) => {
                *status = Some(SyncStatus {
                    phase: "complete".into(),
                    started_at: status.as_ref().map(|s| s.started_at.clone()).unwrap_or_default(),
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
                    phase: "error".into(),
                    started_at: status.as_ref().map(|s| s.started_at.clone()).unwrap_or_default(),
                    finished_at: Some(chrono::Utc::now().to_rfc3339()),
                    is_running: false,
                    error: Some(e.to_string()),
                    loans_synced: 0,
                    payments_synced: 0,
                });
            }
        }
    });

    axum::response::Html(
        r#"<div class="alert alert-info" hx-get="/sync/status" hx-trigger="every 1s" hx-swap="outerHTML">
            Sync started... <span class="loading loading-spinner loading-sm"></span>
        </div>"#
            .to_string(),
    )
}

/// GET /sync/status — HTMX polls this to update the sync status indicator.
async fn sync_status(State(state): State<Arc<AppState>>) -> axum::response::Html<String> {
    let status = state.sync_status.lock().await;

    match status.as_ref() {
        None => axum::response::Html(
            r#"<div class="alert alert-neutral">No sync has been run yet. Click "Run Sync" to start.</div>"#.into(),
        ),
        Some(s) if s.is_running => axum::response::Html(format!(
            r#"<div class="alert alert-info" hx-get="/sync/status" hx-trigger="every 1s" hx-swap="outerHTML">
                <span class="loading loading-spinner loading-sm"></span>
                Syncing: {} ...
            </div>"#,
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
async fn sync_logs_partial(State(state): State<Arc<AppState>>) -> templates::SyncLogsPartialTemplate {
    let logs: Vec<SyncLog> = sqlx::query_as(
        "SELECT id, started_at, finished_at, status, error_message, endpoints_hit,
                events_upserted, loans_upserted, snapshots_created
         FROM sync_log ORDER BY started_at DESC LIMIT 20",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    templates::SyncLogsPartialTemplate { logs }
}
