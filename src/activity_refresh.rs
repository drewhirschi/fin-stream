use std::{str::FromStr, sync::Arc};

use axum::{
    Json,
    extract::Extension,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    crypto::CredentialCipher,
    db::AppContext,
    finance::IsoDate,
    integrations::actions::{MONARCH_CONNECTION_SLUG, MonarchBalanceRequest, sync_monarch_balance},
    operations::{OperationError, OperationRepository},
    sync_runtime::{
        DirectTmoProviderFactory, SystemSyncClock, TMO_CONNECTION_SLUG, TmoSyncService,
    },
};

const REFRESH_AFTER: Duration = Duration::hours(1);

#[derive(Debug, Deserialize)]
pub struct ActivityRefreshRequest {
    as_of_date: String,
}

#[derive(Debug, Serialize)]
struct RefreshResult {
    slug: &'static str,
    outcome: String,
    status: u16,
}

/// Register a best-effort TMO refresh without extending the authenticated
/// page response. The future begins immediately; NextRS keeps it alive through
/// Vercel invocation shutdown and falls back to `tokio::spawn` locally.
pub(crate) fn schedule_tmo_if_stale(
    wait: &nextrs::WaitUntil,
    context: AppContext,
    cipher: Arc<CredentialCipher>,
) {
    if std::env::var("TRUST_DEEDS_ACTIVITY_REFRESH")
        .is_ok_and(|value| matches!(value.as_str(), "0" | "false" | "off"))
    {
        return;
    }
    wait.wait_until(async move {
        refresh_tmo_if_stale(&context, &cipher, OffsetDateTime::now_utc()).await;
    });
}

async fn refresh_tmo_if_stale(
    context: &AppContext,
    cipher: &CredentialCipher,
    now: OffsetDateTime,
) {
    let due = match tmo_is_due(context, now).await {
        Ok(due) => due,
        Err(error) => {
            tracing::error!(%error, "could not determine whether TMO is due for activity refresh");
            return;
        }
    };
    if !due {
        return;
    }

    let connection = match context.connection().await {
        Ok(connection) => connection,
        Err(error) => {
            tracing::error!(%error, "could not load operation control for activity refresh");
            return;
        }
    };
    match OperationRepository::new(&connection)
        .require_writes_enabled()
        .await
    {
        Ok(_) => {}
        Err(OperationError::ReadOnly) => return,
        Err(error) => {
            tracing::error!(%error, "could not validate operation control for activity refresh");
            return;
        }
    }

    let factory = DirectTmoProviderFactory;
    let clock = SystemSyncClock;
    let service = TmoSyncService::production(context, cipher, &factory, &clock);
    match service.run_manual().await {
        Ok(execution) => {
            tracing::info!(
                integration = TMO_CONNECTION_SLUG,
                outcome = execution.outcome_code(),
                "background activity refresh finished"
            );
        }
        Err(error) => {
            tracing::error!(
                integration = TMO_CONNECTION_SLUG,
                failure_class = error.class_code(),
                "background activity refresh could not coordinate integration"
            );
        }
    }
}

/// Refresh configured integrations inline when their last successful refresh
/// is more than one hour old. The POST route is protected by session auth,
/// same-origin validation, and the durable business-write gate. Individual
/// services then use their existing unique-index-backed execution claims, so
/// simultaneous page activity cannot duplicate provider work.
pub async fn post(
    Extension(context): Extension<AppContext>,
    Extension(cipher): Extension<Arc<CredentialCipher>>,
    Json(request): Json<ActivityRefreshRequest>,
) -> Response {
    let as_of_date = match IsoDate::from_str(&request.as_of_date) {
        Ok(value) => value,
        Err(_) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({
                    "error": "validation",
                    "message": "as_of_date must be a valid YYYY-MM-DD calendar date.",
                })),
            )
                .into_response();
        }
    };
    let now = OffsetDateTime::now_utc();
    let due = match due_integrations(&context, now).await {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(%error, "could not determine integrations due for activity refresh");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "service_unavailable" })),
            )
                .into_response();
        }
    };

    let mut results = Vec::with_capacity(due.len());
    if due.contains(&TMO_CONNECTION_SLUG) {
        let factory = DirectTmoProviderFactory;
        let clock = SystemSyncClock;
        let service = TmoSyncService::production(&context, &cipher, &factory, &clock);
        let result = match service.run_manual().await {
            Ok(execution) => RefreshResult {
                slug: TMO_CONNECTION_SLUG,
                outcome: execution.outcome_code().to_owned(),
                status: execution.http_status().as_u16(),
            },
            Err(error) => {
                tracing::error!(
                    integration = TMO_CONNECTION_SLUG,
                    failure_class = error.class_code(),
                    "activity refresh could not coordinate integration"
                );
                RefreshResult {
                    slug: TMO_CONNECTION_SLUG,
                    outcome: error.class_code().to_owned(),
                    status: error.http_status().as_u16(),
                }
            }
        };
        results.push(result);
    }

    if due.contains(&MONARCH_CONNECTION_SLUG) {
        let response = sync_monarch_balance(
            Extension(context),
            Extension(cipher),
            Json(MonarchBalanceRequest {
                as_of_date: as_of_date.to_string(),
            }),
        )
        .await;
        let status = response.status();
        results.push(RefreshResult {
            slug: MONARCH_CONNECTION_SLUG,
            outcome: if status.is_success() {
                "completed".to_owned()
            } else if status == StatusCode::CONFLICT {
                "already_running_or_configuration".to_owned()
            } else {
                "failed".to_owned()
            },
            status: status.as_u16(),
        });
    }

    (
        StatusCode::OK,
        Json(json!({
            "ok": results.iter().all(|result| result.status < 400),
            "refresh_after_seconds": REFRESH_AFTER.whole_seconds(),
            "integrations": results,
        })),
    )
        .into_response()
}

async fn due_integrations(
    context: &AppContext,
    now: OffsetDateTime,
) -> anyhow::Result<Vec<&'static str>> {
    let connection = context.connection().await?;
    let mut rows = connection
        .query(
            "SELECT slug, last_synced_at, \
                    (SELECT MAX(started_at) FROM sync_log \
                     WHERE connection_slug = intg_integration_connection.slug) \
             FROM intg_integration_connection \
             WHERE (slug = 'tmo' AND provider = 'mortgage_office') \
                OR (slug = 'monarch' AND provider = 'monarch') \
             ORDER BY slug",
            (),
        )
        .await?;
    let mut due = Vec::new();
    while let Some(row) = rows.next().await? {
        let slug = row.get::<String>(0)?;
        let last_synced_at = row.get::<Option<String>>(1)?;
        let last_attempted_at = row.get::<Option<String>>(2)?;
        if integration_is_due(last_synced_at.as_deref(), last_attempted_at.as_deref(), now) {
            match slug.as_str() {
                MONARCH_CONNECTION_SLUG => due.push(MONARCH_CONNECTION_SLUG),
                TMO_CONNECTION_SLUG => due.push(TMO_CONNECTION_SLUG),
                _ => {}
            }
        }
    }
    Ok(due)
}

async fn tmo_is_due(context: &AppContext, now: OffsetDateTime) -> anyhow::Result<bool> {
    let connection = context.connection().await?;
    let mut rows = connection
        .query(
            "SELECT last_synced_at, \
                    (SELECT MAX(started_at) FROM sync_log WHERE connection_slug = 'tmo') \
             FROM intg_integration_connection \
             WHERE slug = 'tmo' AND provider = 'mortgage_office'",
            (),
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(false);
    };
    let last_synced_at = row.get::<Option<String>>(0)?;
    let last_attempted_at = row.get::<Option<String>>(1)?;
    Ok(integration_is_due(
        last_synced_at.as_deref(),
        last_attempted_at.as_deref(),
        now,
    ))
}

fn integration_is_due(
    last_synced_at: Option<&str>,
    last_attempted_at: Option<&str>,
    now: OffsetDateTime,
) -> bool {
    [last_synced_at, last_attempted_at]
        .into_iter()
        .flatten()
        .filter_map(|value| OffsetDateTime::parse(value, &Rfc3339).ok())
        .all(|value| value <= now - REFRESH_AFTER)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instant(value: &str) -> OffsetDateTime {
        OffsetDateTime::parse(value, &Rfc3339).unwrap()
    }

    #[test]
    fn activity_refresh_backs_off_from_successes_and_failed_attempts() {
        let now = instant("2026-07-14T20:00:00Z");
        assert!(!integration_is_due(Some("2026-07-14T19:00:01Z"), None, now));
        assert!(!integration_is_due(
            Some("2026-07-14T18:00:00Z"),
            Some("2026-07-14T19:30:00Z"),
            now
        ));
        assert!(integration_is_due(
            Some("2026-07-14T19:00:00Z"),
            Some("2026-07-14T18:30:00Z"),
            now
        ));
        assert!(integration_is_due(None, None, now));
        assert!(integration_is_due(Some("not-a-timestamp"), None, now));
    }
}
