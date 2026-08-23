use std::{error::Error, fmt, sync::Arc};

use axum::{
    Json,
    extract::Extension,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use libsql::TransactionBehavior;
use serde_json::json;
use time::{Duration, OffsetDateTime};

use crate::{
    crypto::CredentialCipher,
    db::AppContext,
    integrations::{
        IntegrationRepository, IntegrationWriteRepository,
        actions::{MONARCH_CONNECTION_SLUG, run_monarch_scheduled},
    },
    operations::{OperationError, OperationRepository, SyncRunStatus},
    sync_runtime::{
        DirectTmoProviderFactory, SyncExecution, SyncRuntimeError, SystemSyncClock,
        TMO_CONNECTION_SLUG, TmoSyncService,
    },
};

const RETRY_AFTER_SECONDS: &str = "60";

/// Fixed cadences retained from the PostgreSQL scheduler. Arbitrary cron
/// expressions are never executed; known legacy expressions are normalized to
/// one of these values on read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncCadence {
    Hourly,
    Every6h,
    Every12h,
    Daily,
    Manual,
}

impl SyncCadence {
    pub const fn default_for_tmo() -> Self {
        Self::Daily
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hourly => "hourly",
            Self::Every6h => "every_6h",
            Self::Every12h => "every_12h",
            Self::Daily => "daily",
            Self::Manual => "manual",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Some(Self::Manual);
        }
        match trimmed.to_ascii_lowercase().as_str() {
            "manual" | "off" | "disabled" => Some(Self::Manual),
            "hourly" | "every_hour" | "1h" => Some(Self::Hourly),
            "every_6h" | "6h" | "4x_daily" | "every_6_hours" => Some(Self::Every6h),
            "every_12h" | "12h" | "2x_daily" | "every_12_hours" => Some(Self::Every12h),
            "daily" | "1x_daily" | "24h" | "every_day" => Some(Self::Daily),
            legacy => Self::from_legacy_cron(legacy),
        }
    }

    fn from_legacy_cron(expression: &str) -> Option<Self> {
        let fields = expression.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 5 {
            return None;
        }
        let hour = fields[1];
        if fields[2..] != ["*", "*", "*"] {
            return Some(Self::Daily);
        }
        match hour {
            "*" => Some(Self::Hourly),
            "*/6" | "0,6,12,18" => Some(Self::Every6h),
            "*/12" | "0,12" => Some(Self::Every12h),
            _ => Some(Self::Daily),
        }
    }

    /// The cadence's repeat interval; `None` for manual-only synchronization.
    pub fn period(self) -> Option<Duration> {
        match self {
            Self::Hourly => Some(Duration::hours(1)),
            Self::Every6h => Some(Duration::hours(6)),
            Self::Every12h => Some(Duration::hours(12)),
            Self::Daily => Some(Duration::days(1)),
            Self::Manual => None,
        }
    }

    /// Most recent deterministic slot at or before `now`. Cron redelivery for
    /// this slot is harmless because `sync_log(connection_slug, scheduled_for)`
    /// is unique. If invocations were absent for multiple slots, the newest
    /// slot collapses those gaps into one full refresh.
    pub fn most_recent_slot(self, now: OffsetDateTime) -> Option<OffsetDateTime> {
        let period_hours = match self {
            Self::Hourly => 1_i64,
            Self::Every6h => 6,
            Self::Every12h => 12,
            Self::Daily => 24,
            Self::Manual => return None,
        };
        let offset_hours = if self == Self::Daily { 6_i64 } else { 0 };
        let period_seconds = period_hours * 60 * 60;
        let offset_seconds = offset_hours * 60 * 60;
        let shifted = now.unix_timestamp() - offset_seconds;
        let slot_seconds = shifted.div_euclid(period_seconds) * period_seconds + offset_seconds;
        OffsetDateTime::from_unix_timestamp(slot_seconds).ok()
    }

    /// Next deterministic slot strictly after `now`.
    pub fn next_slot(self, now: OffsetDateTime) -> Option<OffsetDateTime> {
        let period = match self {
            Self::Hourly => Duration::hours(1),
            Self::Every6h => Duration::hours(6),
            Self::Every12h => Duration::hours(12),
            Self::Daily => Duration::days(1),
            Self::Manual => return None,
        };
        self.most_recent_slot(now)?.checked_add(period)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SchedulePreparation {
    NotConfigured,
    Manual,
    Due {
        scheduled_for: String,
        next_scheduled_at: String,
    },
}

#[derive(Debug)]
enum SchedulerError {
    InvalidCadence,
    Operation(OperationError),
    Storage(anyhow::Error),
}

impl fmt::Display for SchedulerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCadence => formatter.write_str("the TMO sync cadence is invalid"),
            Self::Operation(error) => write!(formatter, "scheduler operation error: {error}"),
            Self::Storage(error) => write!(formatter, "scheduler storage error: {error:#}"),
        }
    }
}

impl Error for SchedulerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidCadence => None,
            Self::Operation(error) => Some(error),
            Self::Storage(error) => Some(error.as_ref()),
        }
    }
}

/// Intentionally mutating GET invoked only by Vercel Cron. Secret auth and the
/// durable scheduler write gate run in the outer middleware before this code.
/// Provider work remains inline; the invocation never spawns or queues work.
pub async fn run_cron(
    Extension(context): Extension<AppContext>,
    Extension(cipher): Extension<Arc<CredentialCipher>>,
) -> Response {
    run_cron_at(context, cipher, OffsetDateTime::now_utc()).await
}

async fn run_cron_at(
    context: AppContext,
    cipher: Arc<CredentialCipher>,
    now: OffsetDateTime,
) -> Response {
    let mut entries = Vec::new();
    let mut worst = StatusCode::OK;
    let mut retry_after = false;
    for slug in [TMO_CONNECTION_SLUG, MONARCH_CONNECTION_SLUG] {
        let preparation = match prepare_schedule(&context, slug, now).await {
            Ok(preparation) => preparation,
            Err(SchedulerError::InvalidCadence) => {
                entries.push(json!({
                    "provider": slug,
                    "outcome": "invalid_cadence",
                }));
                worst = worst.max(StatusCode::CONFLICT);
                continue;
            }
            Err(SchedulerError::Operation(OperationError::ReadOnly)) => {
                return cron_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "read_only",
                    "Writes are temporarily disabled.",
                    true,
                );
            }
            Err(SchedulerError::Operation(OperationError::SchedulerDisabled)) => {
                return cron_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "scheduler_disabled",
                    "Scheduled synchronization is disabled.",
                    true,
                );
            }
            Err(SchedulerError::Operation(error)) => {
                tracing::error!(%error, "cron could not recheck operation control");
                return cron_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "service_unavailable",
                    "Scheduled synchronization is temporarily unavailable.",
                    true,
                );
            }
            Err(SchedulerError::Storage(error)) => {
                tracing::error!(%error, slug, "cron could not prepare the schedule");
                return cron_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "service_unavailable",
                    "Scheduled synchronization is temporarily unavailable.",
                    true,
                );
            }
        };
        let (scheduled_for, next_scheduled_at) = match preparation {
            SchedulePreparation::NotConfigured => {
                entries.push(json!({ "provider": slug, "outcome": "not_configured" }));
                continue;
            }
            SchedulePreparation::Manual => {
                entries.push(json!({ "provider": slug, "outcome": "manual" }));
                continue;
            }
            SchedulePreparation::Due {
                scheduled_for,
                next_scheduled_at,
            } => (scheduled_for, next_scheduled_at),
        };

        let (status, needs_retry, mut entry) = match slug {
            TMO_CONNECTION_SLUG => {
                let provider_factory = DirectTmoProviderFactory;
                let clock = SystemSyncClock;
                let service =
                    TmoSyncService::production(&context, &cipher, &provider_factory, &clock);
                match service.run_scheduled(&scheduled_for).await {
                    Ok(execution) => tmo_execution_entry(execution),
                    Err(error) => {
                        tracing::error!(
                            failure_class = error.class_code(),
                            "cron could not coordinate the TMO synchronization"
                        );
                        (
                            error.http_status(),
                            matches!(error, SyncRuntimeError::Storage(_)),
                            json!({
                                "outcome": "failed",
                                "error": error.class_code(),
                                "message": error.public_message(),
                            }),
                        )
                    }
                }
            }
            _ => {
                let (status, entry) =
                    run_monarch_scheduled(&context, &cipher, &scheduled_for).await;
                (status, status == StatusCode::SERVICE_UNAVAILABLE, entry)
            }
        };
        if let Some(object) = entry.as_object_mut() {
            object.insert("provider".into(), json!(slug));
            object.insert("scheduled_for".into(), json!(scheduled_for));
            object.insert("next_scheduled_at".into(), json!(next_scheduled_at));
        }
        entries.push(entry);
        worst = worst.max(status);
        retry_after |= needs_retry;
    }

    let mut response = (worst, Json(json!({ "integrations": entries }))).into_response();
    if retry_after {
        response.headers_mut().insert(
            header::RETRY_AFTER,
            HeaderValue::from_static(RETRY_AFTER_SECONDS),
        );
    }
    response
}

async fn prepare_schedule(
    context: &AppContext,
    slug: &str,
    now: OffsetDateTime,
) -> Result<SchedulePreparation, SchedulerError> {
    let connection = context
        .connection()
        .await
        .map_err(SchedulerError::Storage)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .await
        .map_err(|error| SchedulerError::Storage(error.into()))?;
    OperationRepository::new(&transaction)
        .require_scheduler_enabled()
        .await
        .map_err(SchedulerError::Operation)?;
    let integration = IntegrationRepository::new(&transaction)
        .connection_by_slug(slug)
        .await
        .map_err(|error| SchedulerError::Storage(error.into()))?;
    let Some(integration) = integration else {
        transaction
            .commit()
            .await
            .map_err(|error| SchedulerError::Storage(error.into()))?;
        return Ok(SchedulePreparation::NotConfigured);
    };

    let cadence = SyncCadence::parse(&integration.sync_cadence);
    let next = cadence.and_then(|cadence| cadence.next_slot(now));
    let updated_at = format_utc_millis(now);
    let next_scheduled_at = next.map(format_utc_millis);
    IntegrationWriteRepository::new(&transaction)
        .set_next_scheduled_at(slug, next_scheduled_at.as_deref(), &updated_at)
        .await
        .map_err(|error| SchedulerError::Storage(error.into()))?;
    transaction
        .commit()
        .await
        .map_err(|error| SchedulerError::Storage(error.into()))?;

    let Some(cadence) = cadence else {
        return Err(SchedulerError::InvalidCadence);
    };
    if cadence == SyncCadence::Manual {
        return Ok(SchedulePreparation::Manual);
    }
    let scheduled_for = cadence
        .most_recent_slot(now)
        .map(format_utc_millis)
        .ok_or_else(|| {
            SchedulerError::Storage(anyhow::anyhow!("could not calculate the current UTC slot"))
        })?;
    let next_scheduled_at = next_scheduled_at.ok_or_else(|| {
        SchedulerError::Storage(anyhow::anyhow!("could not calculate the next UTC slot"))
    })?;
    Ok(SchedulePreparation::Due {
        scheduled_for,
        next_scheduled_at,
    })
}

fn tmo_execution_entry(execution: SyncExecution) -> (StatusCode, bool, serde_json::Value) {
    let (status, outcome, retry_policy, retry_after) = match &execution {
        SyncExecution::Completed(_) => (StatusCode::OK, "completed", "none", false),
        SyncExecution::Failed { .. } => (
            execution.http_status(),
            "failed",
            "next_scheduled_slot_or_manual",
            false,
        ),
        SyncExecution::AlreadyRunning(_) => (
            StatusCode::ACCEPTED,
            "already_running",
            "safe_redelivery_after_retry_after",
            true,
        ),
        SyncExecution::AlreadyScheduled(run) if run.status == SyncRunStatus::Running => (
            StatusCode::ACCEPTED,
            "slot_already_running",
            "safe_redelivery_after_retry_after",
            true,
        ),
        SyncExecution::AlreadyScheduled(run) if run.status == SyncRunStatus::Error => (
            StatusCode::OK,
            "failed_slot_already_recorded",
            "next_scheduled_slot_or_manual",
            false,
        ),
        SyncExecution::AlreadyScheduled(_) => {
            (StatusCode::OK, "slot_already_completed", "none", false)
        }
        SyncExecution::CoveredBySuccess(_) => (StatusCode::OK, "covered_by_success", "none", false),
    };
    (
        status,
        retry_after,
        json!({
            "outcome": outcome,
            "retry_policy": retry_policy,
            "run": execution.run(),
        }),
    )
}

#[cfg(all(test, feature = "local-db"))]
fn cron_execution_response(
    execution: SyncExecution,
    scheduled_for: &str,
    next_scheduled_at: &str,
) -> Response {
    let (status, retry_after, mut entry) = tmo_execution_entry(execution);
    if let Some(object) = entry.as_object_mut() {
        object.insert("provider".into(), json!(TMO_CONNECTION_SLUG));
        object.insert("scheduled_for".into(), json!(scheduled_for));
        object.insert("next_scheduled_at".into(), json!(next_scheduled_at));
    }
    let mut response = (status, Json(entry)).into_response();
    if retry_after {
        response.headers_mut().insert(
            header::RETRY_AFTER,
            HeaderValue::from_static(RETRY_AFTER_SECONDS),
        );
    }
    response
}

fn cron_error(status: StatusCode, code: &str, message: &str, retry_after: bool) -> Response {
    let mut response = (
        status,
        Json(json!({
            "error": code,
            "message": message,
        })),
    )
        .into_response();
    if retry_after {
        response.headers_mut().insert(
            header::RETRY_AFTER,
            HeaderValue::from_static(RETRY_AFTER_SECONDS),
        );
    }
    response
}

fn format_utc_millis(value: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        value.year(),
        value.month() as u8,
        value.day(),
        value.hour(),
        value.minute(),
        value.second(),
        value.millisecond(),
    )
}

#[cfg(all(test, feature = "local-db"))]
mod tests {
    use axum::body::to_bytes;
    use libsql::{Builder, params};
    use time::{Date, Month, Time};

    use super::*;
    use crate::{operations::SyncRun, sync_runtime::SyncFailureClass};

    fn at(year: i32, month: Month, day: u8, hour: u8, minute: u8) -> OffsetDateTime {
        Date::from_calendar_date(year, month, day)
            .unwrap()
            .with_time(Time::from_hms(hour, minute, 0).unwrap())
            .assume_utc()
    }

    async fn context_with_cadence(cadence: &str) -> AppContext {
        let database = Builder::new_local(":memory:").build().await.unwrap();
        let context = AppContext::from_database(database).await.unwrap();
        context
            .connection()
            .await
            .unwrap()
            .execute(
                "INSERT INTO intg_integration_connection (slug, name, provider, sync_cadence) \
                 VALUES ('tmo', 'The Mortgage Office', 'mortgage_office', ?1)",
                params![cadence],
            )
            .await
            .unwrap();
        let connection = context.connection().await.unwrap();
        let operations = OperationRepository::new(&connection);
        operations
            .enable_writes("2026-07-14T13:00:00.000Z")
            .await
            .unwrap();
        operations
            .set_scheduler_enabled(true, "2026-07-14T13:00:01.000Z")
            .await
            .unwrap();
        context
    }

    #[test]
    fn parses_canonical_alias_and_legacy_cadences() {
        assert_eq!(SyncCadence::parse("hourly"), Some(SyncCadence::Hourly));
        assert_eq!(SyncCadence::parse("6h"), Some(SyncCadence::Every6h));
        assert_eq!(SyncCadence::parse("2x_daily"), Some(SyncCadence::Every12h));
        assert_eq!(SyncCadence::parse("24h"), Some(SyncCadence::Daily));
        assert_eq!(SyncCadence::parse(""), Some(SyncCadence::Manual));
        assert_eq!(
            SyncCadence::parse("0 0,6,12,18 * * *"),
            Some(SyncCadence::Every6h)
        );
        assert_eq!(SyncCadence::parse("0 21 * * *"), Some(SyncCadence::Daily));
        assert_eq!(SyncCadence::parse("sometimes"), None);
    }

    #[test]
    fn slots_are_deterministic_at_and_between_boundaries() {
        let now = at(2026, Month::July, 14, 13, 25);
        assert_eq!(
            format_utc_millis(SyncCadence::Hourly.most_recent_slot(now).unwrap()),
            "2026-07-14T13:00:00.000Z"
        );
        assert_eq!(
            format_utc_millis(SyncCadence::Every6h.most_recent_slot(now).unwrap()),
            "2026-07-14T12:00:00.000Z"
        );
        assert_eq!(
            format_utc_millis(SyncCadence::Every6h.next_slot(now).unwrap()),
            "2026-07-14T18:00:00.000Z"
        );
        assert_eq!(
            format_utc_millis(
                SyncCadence::Every12h
                    .most_recent_slot(at(2026, Month::July, 14, 0, 0))
                    .unwrap()
            ),
            "2026-07-14T00:00:00.000Z"
        );
        assert_eq!(
            format_utc_millis(
                SyncCadence::Daily
                    .most_recent_slot(at(2026, Month::July, 14, 5, 59))
                    .unwrap()
            ),
            "2026-07-13T06:00:00.000Z"
        );
        assert_eq!(
            format_utc_millis(
                SyncCadence::Daily
                    .most_recent_slot(at(2026, Month::July, 14, 6, 0))
                    .unwrap()
            ),
            "2026-07-14T06:00:00.000Z"
        );
        assert!(SyncCadence::Manual.most_recent_slot(now).is_none());
    }

    #[tokio::test]
    async fn preparation_persists_the_truthful_next_slot_transactionally() {
        let context = context_with_cadence("every_6h").await;
        let decision = prepare_schedule(&context, "tmo", at(2026, Month::July, 14, 13, 25))
            .await
            .unwrap();
        assert_eq!(
            decision,
            SchedulePreparation::Due {
                scheduled_for: "2026-07-14T12:00:00.000Z".into(),
                next_scheduled_at: "2026-07-14T18:00:00.000Z".into(),
            }
        );
        let integration = IntegrationRepository::new(&context.connection().await.unwrap())
            .connection_by_slug("tmo")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            integration.next_scheduled_at.as_deref(),
            Some("2026-07-14T18:00:00.000Z")
        );
    }

    #[tokio::test]
    async fn manual_and_invalid_cadences_clear_a_stale_next_slot() {
        for (cadence, is_error) in [("manual", false), ("not-a-cadence", true)] {
            let context = context_with_cadence(cadence).await;
            context
                .connection()
                .await
                .unwrap()
                .execute(
                    "UPDATE intg_integration_connection \
                     SET next_scheduled_at = '2026-07-14T18:00:00.000Z' WHERE slug = 'tmo'",
                    (),
                )
                .await
                .unwrap();
            let result = prepare_schedule(&context, "tmo", at(2026, Month::July, 14, 13, 25)).await;
            assert_eq!(result.is_err(), is_error);
            if !is_error {
                assert_eq!(result.unwrap(), SchedulePreparation::Manual);
            }
            let integration = IntegrationRepository::new(&context.connection().await.unwrap())
                .connection_by_slug("tmo")
                .await
                .unwrap()
                .unwrap();
            assert_eq!(integration.next_scheduled_at, None);
        }
    }

    fn scheduled_run(status: SyncRunStatus) -> SyncRun {
        SyncRun {
            id: 41,
            connection_slug: "tmo".into(),
            scheduled_for: Some("2026-07-14T12:00:00.000Z".into()),
            started_at: "2026-07-14T12:00:01.000Z".into(),
            finished_at: (status != SyncRunStatus::Running)
                .then(|| "2026-07-14T12:00:02.000Z".into()),
            status,
            error_message: (status == SyncRunStatus::Error)
                .then(|| "TMO could not complete the synchronization request.".into()),
            endpoints_hit: None,
            events_upserted: 0,
            loans_upserted: 0,
            snapshots_created: 0,
        }
    }

    #[tokio::test]
    async fn failed_slot_responses_state_the_next_slot_or_manual_retry_policy() {
        let failed = cron_execution_response(
            SyncExecution::Failed {
                run: scheduled_run(SyncRunStatus::Error),
                class: SyncFailureClass::Provider,
            },
            "2026-07-14T12:00:00.000Z",
            "2026-07-14T18:00:00.000Z",
        );
        assert_eq!(failed.status(), StatusCode::BAD_GATEWAY);
        assert!(failed.headers().get(header::RETRY_AFTER).is_none());
        let body = to_bytes(failed.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["outcome"], "failed");
        assert_eq!(body["retry_policy"], "next_scheduled_slot_or_manual");

        let redelivery = cron_execution_response(
            SyncExecution::AlreadyScheduled(scheduled_run(SyncRunStatus::Error)),
            "2026-07-14T12:00:00.000Z",
            "2026-07-14T18:00:00.000Z",
        );
        assert_eq!(redelivery.status(), StatusCode::OK);
        let body = to_bytes(redelivery.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["outcome"], "failed_slot_already_recorded");
        assert_eq!(body["retry_policy"], "next_scheduled_slot_or_manual");
    }

    #[tokio::test]
    async fn running_slot_response_is_retryable_without_claiming_again() {
        let response = cron_execution_response(
            SyncExecution::AlreadyScheduled(scheduled_run(SyncRunStatus::Running)),
            "2026-07-14T12:00:00.000Z",
            "2026-07-14T18:00:00.000Z",
        );
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(response.headers()[header::RETRY_AFTER], RETRY_AFTER_SECONDS);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["outcome"], "slot_already_running");
    }

    #[tokio::test]
    async fn cron_reports_every_integration_and_noops_when_nothing_is_due() {
        let context = context_with_cadence("manual").await;
        let cipher = Arc::new(CredentialCipher::new("scheduler-test-key").unwrap());
        let response = run_cron_at(
            context,
            cipher,
            at(2026, Month::July, 14, 13, 25),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let entries = body["integrations"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["provider"], "tmo");
        assert_eq!(entries[0]["outcome"], "manual");
        assert_eq!(entries[1]["provider"], "monarch");
        assert_eq!(entries[1]["outcome"], "not_configured");
    }
}
