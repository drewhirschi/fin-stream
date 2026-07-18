use std::sync::Arc;

use askama::Template;
use axum::{
    Json,
    extract::{Extension, Path, Query},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    crypto::CredentialCipher,
    db::AppContext,
    filters,
    operations::{OperationRepository, SyncRun, SyncRunStatus},
};

use super::{
    DirectTmoProviderFactory, SyncExecution, SyncRuntimeError, SystemSyncClock,
    TMO_CONNECTION_SLUG, TmoSyncService, current_or_last,
};

#[derive(Debug, Default, Deserialize)]
pub struct SyncQuery {
    slug: Option<String>,
}

pub async fn run_global(
    Extension(context): Extension<AppContext>,
    Extension(cipher): Extension<Arc<CredentialCipher>>,
    Query(query): Query<SyncQuery>,
    headers: HeaderMap,
) -> Response {
    run_for_slug(
        context,
        cipher,
        query.slug.as_deref().unwrap_or(TMO_CONNECTION_SLUG),
        &headers,
    )
    .await
}

pub async fn run_scoped(
    Extension(context): Extension<AppContext>,
    Extension(cipher): Extension<Arc<CredentialCipher>>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Response {
    run_for_slug(context, cipher, &slug, &headers).await
}

pub async fn status_global(
    Extension(context): Extension<AppContext>,
    Query(query): Query<SyncQuery>,
    headers: HeaderMap,
) -> Response {
    status_for_slug(
        context,
        query.slug.as_deref().unwrap_or(TMO_CONNECTION_SLUG),
        &headers,
    )
    .await
}

pub async fn status_scoped(
    Extension(context): Extension<AppContext>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Response {
    status_for_slug(context, &slug, &headers).await
}

pub async fn logs_global(
    Extension(context): Extension<AppContext>,
    Query(query): Query<SyncQuery>,
    headers: HeaderMap,
) -> Response {
    logs_for_slug(
        context,
        query.slug.as_deref().unwrap_or(TMO_CONNECTION_SLUG),
        &headers,
    )
    .await
}

pub async fn logs_scoped(
    Extension(context): Extension<AppContext>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Response {
    logs_for_slug(context, &slug, &headers).await
}

async fn run_for_slug(
    context: AppContext,
    cipher: Arc<CredentialCipher>,
    slug: &str,
    headers: &HeaderMap,
) -> Response {
    if slug != TMO_CONNECTION_SLUG {
        return simple_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unsupported_integration",
            "Synchronization is not available for this integration.",
            headers,
        );
    }

    let factory = DirectTmoProviderFactory;
    let clock = SystemSyncClock;
    let service = TmoSyncService::production(&context, &cipher, &factory, &clock);
    match service.run_manual().await {
        Ok(execution) => execution_response(&execution, headers),
        Err(error) => runtime_error_response(&error, headers),
    }
}

async fn status_for_slug(context: AppContext, slug: &str, headers: &HeaderMap) -> Response {
    if slug != TMO_CONNECTION_SLUG {
        return simple_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unsupported_integration",
            "Synchronization is not available for this integration.",
            headers,
        );
    }
    match recent_runs(&context, slug).await {
        Ok(runs) => {
            let run = current_or_last(&runs);
            if wants_json(headers) {
                (StatusCode::OK, Json(json!({ "run": run }))).into_response()
            } else {
                let outcome = run.as_ref().map_or("none", |run| match run.status {
                    SyncRunStatus::Running => "running",
                    SyncRunStatus::Success => "completed",
                    SyncRunStatus::Error => "failed",
                });
                render_status(StatusCode::OK, outcome, run.as_ref())
            }
        }
        Err(error) => runtime_error_response(&error, headers),
    }
}

async fn logs_for_slug(context: AppContext, slug: &str, headers: &HeaderMap) -> Response {
    if slug != TMO_CONNECTION_SLUG {
        return simple_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unsupported_integration",
            "Synchronization is not available for this integration.",
            headers,
        );
    }
    match recent_runs(&context, slug).await {
        Ok(runs) if wants_json(headers) => {
            (StatusCode::OK, Json(json!({ "runs": runs }))).into_response()
        }
        Ok(runs) => render_logs(StatusCode::OK, &runs),
        Err(error) => runtime_error_response(&error, headers),
    }
}

async fn recent_runs(context: &AppContext, slug: &str) -> Result<Vec<SyncRun>, SyncRuntimeError> {
    let connection = context
        .connection()
        .await
        .map_err(SyncRuntimeError::Storage)?;
    OperationRepository::new(&connection)
        .list_recent(slug, 20)
        .await
        .map_err(SyncRuntimeError::Operation)
}

fn execution_response(execution: &SyncExecution, headers: &HeaderMap) -> Response {
    let status = execution.http_status();
    if wants_json(headers) {
        return (
            status,
            Json(json!({
                "outcome": execution.outcome_code(),
                "run": execution.run(),
            })),
        )
            .into_response();
    }
    // HTMX swaps successful responses by default. Its fragment reports the
    // durable failed/conflict outcome; ordinary HTML and JSON callers retain
    // the corresponding 409/502 status for automation.
    render_status(
        if is_htmx(headers) {
            StatusCode::OK
        } else {
            status
        },
        execution.outcome_code(),
        Some(execution.run()),
    )
}

fn runtime_error_response(error: &SyncRuntimeError, headers: &HeaderMap) -> Response {
    simple_error(
        error.http_status(),
        error.class_code(),
        error.public_message(),
        headers,
    )
}

fn simple_error(status: StatusCode, code: &str, message: &str, headers: &HeaderMap) -> Response {
    if wants_json(headers) {
        return (status, Json(json!({ "error": code, "message": message }))).into_response();
    }
    let class = if status.is_server_error() {
        "alert-error"
    } else {
        "alert-warning"
    };
    render_fragment(status, ErrorTemplate { class, message }, "sync error")
}

fn render_status(status: StatusCode, outcome: &str, run: Option<&SyncRun>) -> Response {
    render_fragment(status, StatusTemplate { outcome, run }, "sync status")
}

fn render_logs(status: StatusCode, runs: &[SyncRun]) -> Response {
    render_fragment(status, LogsTemplate { runs }, "sync logs")
}

fn render_fragment(status: StatusCode, template: impl Template, label: &'static str) -> Response {
    match template.render() {
        Ok(html) => (status, Html(html)).into_response(),
        Err(error) => {
            tracing::error!(%error, template = label, "failed to render sync fragment");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not render synchronization status.",
            )
                .into_response()
        }
    }
}

fn wants_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("application/json"))
}

fn is_htmx(headers: &HeaderMap) -> bool {
    headers
        .get("HX-Request")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

#[derive(Template)]
#[template(
    source = r#"
{% match run %}
{% when None %}<div class="alert alert-neutral">No sync has been run yet.</div>
{% when Some with (run) %}
{% if outcome == "already_running" || outcome == "running" %}
<div class="alert alert-info" hx-get="/sync/status" hx-trigger="every 2s" hx-swap="outerHTML"><span class="loading loading-spinner loading-sm"></span><span>Sync is already running (started {{ run.started_at|datetime_local|safe }}).</span></div>
{% else if outcome == "already_scheduled" %}
<div class="alert alert-warning">This scheduled sync slot has already been claimed.</div>
{% else if outcome == "covered_by_success" %}
<div class="alert alert-success">A successful sync already covers this scheduled slot.</div>
{% else if outcome == "failed" %}
<div class="alert alert-error"><span>Sync failed: {{ run.error_message.as_deref().unwrap_or("The provider request could not be completed.") }}</span></div>
{% else %}
<div class="alert alert-success"><span>Sync complete — {{ run.loans_upserted|number }} loans, {{ run.events_upserted|number }} events, and {{ run.snapshots_created|number }} snapshot records updated.</span></div>
{% endif %}
{% endmatch %}
"#,
    ext = "html"
)]
struct StatusTemplate<'a> {
    outcome: &'a str,
    run: Option<&'a SyncRun>,
}

#[derive(Template)]
#[template(
    source = r#"
{% if runs.is_empty() %}<tr><td colspan="9" class="label-dim">No sync executions have been recorded.</td></tr>{% else %}
{% for run in runs %}<tr>
<td>{{ run.id|number }}</td><td>{{ run.started_at|datetime_local|safe }}</td><td>{% match run.finished_at %}{% when Some with (timestamp) %}{{ timestamp|datetime_local|safe }}{% when None %}—{% endmatch %}</td>
<td>{{ run.status.as_str() }}</td><td>{{ run.loans_upserted|number }}</td><td>{{ run.events_upserted|number }}</td><td>{{ run.snapshots_created|number }}</td><td>{{ run.endpoints_hit.as_deref().unwrap_or("—") }}</td><td>{{ run.error_message.as_deref().unwrap_or("—") }}</td>
</tr>{% endfor %}{% endif %}
"#,
    ext = "html"
)]
struct LogsTemplate<'a> {
    runs: &'a [SyncRun],
}

#[derive(Template)]
#[template(
    source = r#"<div class="alert {{ class }}"><span>{{ message }}</span></div>"#,
    ext = "html"
)]
struct ErrorTemplate<'a> {
    class: &'a str,
    message: &'a str,
}

#[cfg(test)]
mod tests {
    use axum::{body::to_bytes, http::HeaderValue};

    use crate::operations::{SyncRun, SyncRunStatus};

    use super::*;
    use crate::sync_runtime::SyncFailureClass;

    fn failed_execution() -> SyncExecution {
        SyncExecution::Failed {
            run: SyncRun {
                id: 12_345,
                connection_slug: "tmo".into(),
                scheduled_for: None,
                started_at: "2026-07-14T12:00:00.000Z".into(),
                finished_at: Some("2026-07-14T12:00:01.000Z".into()),
                status: SyncRunStatus::Error,
                error_message: Some("TMO rejected authentication.".into()),
                endpoints_hit: None,
                events_upserted: 0,
                loans_upserted: 0,
                snapshots_created: 0,
            },
            class: SyncFailureClass::Provider,
        }
    }

    #[tokio::test]
    async fn json_keeps_error_status_while_htmx_gets_a_swappable_truthful_fragment() {
        let mut json_headers = HeaderMap::new();
        json_headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        let json_response = execution_response(&failed_execution(), &json_headers);
        assert_eq!(json_response.status(), StatusCode::BAD_GATEWAY);

        let ordinary_response = execution_response(&failed_execution(), &HeaderMap::new());
        assert_eq!(ordinary_response.status(), StatusCode::BAD_GATEWAY);

        let mut htmx_headers = HeaderMap::new();
        htmx_headers.insert("HX-Request", HeaderValue::from_static("true"));
        let htmx_response = execution_response(&failed_execution(), &htmx_headers);
        assert_eq!(htmx_response.status(), StatusCode::OK);
        let body = to_bytes(htmx_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("Sync failed: TMO rejected authentication."));
    }

    #[test]
    fn status_fragments_use_shared_count_and_datetime_filters() {
        let run = failed_execution().run().clone();
        let html = StatusTemplate {
            outcome: "already_running",
            run: Some(&run),
        }
        .render()
        .unwrap();
        assert!(html.contains("12:00 PM UTC"));

        let log_html = LogsTemplate { runs: &[run] }.render().unwrap();
        assert!(log_html.contains("12,345"));
        assert!(log_html.contains("data-local="));
    }
}
