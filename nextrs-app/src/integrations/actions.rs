//! Small, durable integration mutations that do not belong to the full TMO
//! import runtime.
//!
//! Both actions are safe for a serverless runtime: cadence is persisted in one
//! short transaction, while Monarch provider work happens after the durable
//! execution claim and before the short completion transaction.

use std::{error::Error, fmt, str::FromStr, sync::Arc};

use anyhow::Context;
use askama::Template;
use async_trait::async_trait;
use axum::{
    Form, Json,
    extract::{Extension, Path},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use libsql::{Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::{Duration, OffsetDateTime};

use crate::{
    crypto::{CredentialCipher, CredentialCryptoError},
    db::AppContext,
    finance::IsoDate,
    operations::{ClaimOutcome, OperationError, OperationRepository, SyncCompletion, SyncRun},
    providers::{
        ProviderError, ProviderResult,
        monarch::{AccountBalance, MonarchClient},
    },
    scheduler::SyncCadence,
    sync_runtime::{SyncClock, SystemSyncClock, TMO_CONNECTION_SLUG},
};

use super::{IntegrationError, IntegrationRepository};

pub const MONARCH_CONNECTION_SLUG: &str = "monarch";

const RETRY_AFTER_SECONDS: &str = "60";
const MAX_ABS_BALANCE: f64 = 1.0e308;
const MAX_TOKEN_BYTES: usize = 16 * 1_024;
const MAX_ACCOUNT_ID_BYTES: usize = 256;
const MAX_ACCOUNT_NAME_BYTES: usize = 256;
const MAX_MASK_BYTES: usize = 64;
const MAX_PROVIDER_TIMESTAMP_BYTES: usize = 64;
const MAX_METADATA_BYTES: usize = 16 * 1_024;
// This must remain above the deployed function duration plus clock/network
// skew. It intentionally matches the conservative TMO stale boundary.
const STALE_AFTER: Duration = Duration::minutes(20);

#[derive(Debug, Deserialize)]
pub struct CadenceRequest {
    pub sync_cadence: String,
}

#[derive(Debug, Deserialize)]
pub struct MonarchBalanceRequest {
    pub as_of_date: String,
}

/// Persist a canonical cadence and its truthful next UTC slot. Legacy aliases
/// are accepted on read by the scheduler, but this write API never creates
/// more of them.
pub async fn update_cadence(
    Extension(context): Extension<AppContext>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Form(request): Form<CadenceRequest>,
) -> Response {
    let Some(cadence) = canonical_cadence(&request.sync_cadence) else {
        return cadence_error_response(CadenceMutationError::InvalidCadence, &headers);
    };
    if slug != TMO_CONNECTION_SLUG {
        return cadence_error_response(CadenceMutationError::UnsupportedIntegration, &headers);
    }
    let now = OffsetDateTime::now_utc();
    match persist_cadence(&context, &slug, cadence, now).await {
        Ok(next_scheduled_at) => {
            cadence_success_response(cadence, next_scheduled_at.as_deref(), &headers)
        }
        Err(error) => cadence_error_response(error, &headers),
    }
}

fn canonical_cadence(raw: &str) -> Option<SyncCadence> {
    let cadence = SyncCadence::parse(raw)?;
    (raw == cadence.as_str()).then_some(cadence)
}

async fn persist_cadence(
    context: &AppContext,
    slug: &str,
    cadence: SyncCadence,
    now: OffsetDateTime,
) -> Result<Option<String>, CadenceMutationError> {
    if slug != TMO_CONNECTION_SLUG {
        return Err(CadenceMutationError::UnsupportedIntegration);
    }
    let connection = context
        .connection()
        .await
        .map_err(CadenceMutationError::Storage)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .await
        .map_err(|error| CadenceMutationError::Storage(error.into()))?;
    let updated_at = format_utc_millis(now);
    let next_scheduled_at = cadence.next_slot(now).map(format_utc_millis);
    let changed = transaction
        .execute(
            "UPDATE intg_integration_connection \
             SET sync_cadence = ?2, next_scheduled_at = ?3, updated_at = ?4 \
             WHERE slug = ?1",
            params![
                slug,
                cadence.as_str(),
                next_scheduled_at.clone(),
                updated_at,
            ],
        )
        .await
        .context("update integration cadence")
        .map_err(CadenceMutationError::Storage)?;
    if changed != 1 {
        return Err(CadenceMutationError::NotFound);
    }
    transaction
        .commit()
        .await
        .context("commit integration cadence")
        .map_err(CadenceMutationError::Storage)?;
    Ok(next_scheduled_at)
}

#[derive(Debug)]
enum CadenceMutationError {
    InvalidCadence,
    UnsupportedIntegration,
    NotFound,
    Storage(anyhow::Error),
}

impl CadenceMutationError {
    const fn status(&self) -> StatusCode {
        match self {
            Self::InvalidCadence | Self::UnsupportedIntegration => StatusCode::UNPROCESSABLE_ENTITY,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Storage(_) => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    const fn code(&self) -> &'static str {
        match self {
            Self::InvalidCadence => "invalid_cadence",
            Self::UnsupportedIntegration => "unsupported_integration",
            Self::NotFound => "not_found",
            Self::Storage(_) => "service_unavailable",
        }
    }

    const fn message(&self) -> &'static str {
        match self {
            Self::InvalidCadence => {
                "Choose manual, hourly, every 6 hours, every 12 hours, or daily."
            }
            Self::UnsupportedIntegration => {
                "Automatic synchronization is not available for this integration."
            }
            Self::NotFound => "This integration no longer exists.",
            Self::Storage(_) => "The sync cadence could not be saved. Try again.",
        }
    }
}

fn cadence_success_response(
    cadence: SyncCadence,
    next_scheduled_at: Option<&str>,
    headers: &HeaderMap,
) -> Response {
    if wants_json(headers) {
        return (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "sync_cadence": cadence.as_str(),
                "next_scheduled_at": next_scheduled_at,
            })),
        )
            .into_response();
    }
    render_cadence_feedback(
        StatusCode::OK,
        "alert-success",
        "Sync cadence saved.",
        Some(cadence.as_str()),
        headers,
        false,
    )
}

fn cadence_error_response(error: CadenceMutationError, headers: &HeaderMap) -> Response {
    if let CadenceMutationError::Storage(source) = &error {
        tracing::error!(%source, "could not persist integration cadence");
    }
    let status = error.status();
    let retry_after = status == StatusCode::SERVICE_UNAVAILABLE;
    if wants_json(headers) {
        let mut response = (
            status,
            Json(json!({ "error": error.code(), "message": error.message() })),
        )
            .into_response();
        if retry_after {
            response.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from_static(RETRY_AFTER_SECONDS),
            );
        }
        return response;
    }
    let class = if status.is_server_error() {
        "alert-error"
    } else {
        "alert-warning"
    };
    render_cadence_feedback(status, class, error.message(), None, headers, retry_after)
}

fn render_cadence_feedback(
    status: StatusCode,
    class: &'static str,
    message: &'static str,
    cadence: Option<&str>,
    headers: &HeaderMap,
    retry_after: bool,
) -> Response {
    let rendered = CadenceFeedbackTemplate {
        class,
        message,
        cadence,
    }
    .render();
    let mut response = match rendered {
        Ok(html) => (
            // HTMX only swaps 2xx responses by default. The fragment still
            // states the truthful outcome; ordinary HTML and JSON retain the
            // real status for automation and diagnostics.
            if is_htmx(headers) {
                StatusCode::OK
            } else {
                status
            },
            Html(html),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(%error, "could not render cadence feedback");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not render cadence status.",
            )
                .into_response()
        }
    };
    if retry_after {
        response.headers_mut().insert(
            header::RETRY_AFTER,
            HeaderValue::from_static(RETRY_AFTER_SECONDS),
        );
    }
    response
}

#[derive(Template)]
#[template(
    source = r#"<div class="alert {{ class }} py-2 text-sm"><span>{{ message }}</span></div>{% match cadence %}{% when Some with (value) %}<span id="sync-cadence-current" hx-swap-oob="innerHTML">{{ value }}</span>{% when None %}{% endmatch %}"#,
    ext = "html"
)]
struct CadenceFeedbackTemplate<'a> {
    class: &'a str,
    message: &'a str,
    cadence: Option<&'a str>,
}

/// Refresh the cash anchor from the imported Monarch credential. A browser
/// local date is required because a Vercel region's calendar date is not the
/// user's calendar date.
pub async fn sync_monarch_balance(
    Extension(context): Extension<AppContext>,
    Extension(cipher): Extension<Arc<CredentialCipher>>,
    Json(request): Json<MonarchBalanceRequest>,
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
    let provider = DirectMonarchBalanceProvider;
    let clock = SystemSyncClock;
    let service = MonarchBalanceService::with_dependencies(&context, &cipher, &provider, &clock);
    match service.run(as_of_date).await {
        Ok(execution) => monarch_execution_response(execution),
        Err(error) => monarch_error_response(error),
    }
}

#[derive(Clone, Debug)]
struct MonarchBalanceCapture {
    account: AccountBalance,
    adjusted_balance: f64,
    pending_total: f64,
}

#[async_trait]
trait MonarchBalanceProvider: Send + Sync {
    async fn capture(
        &self,
        access_token: &str,
        account_id: &str,
    ) -> ProviderResult<MonarchBalanceCapture>;
}

#[derive(Clone, Copy, Debug, Default)]
struct DirectMonarchBalanceProvider;

#[async_trait]
impl MonarchBalanceProvider for DirectMonarchBalanceProvider {
    async fn capture(
        &self,
        access_token: &str,
        account_id: &str,
    ) -> ProviderResult<MonarchBalanceCapture> {
        let client = MonarchClient::with_token(access_token)?;
        let (account, adjusted_balance, pending_total) =
            client.get_adjusted_balance(account_id).await?;
        Ok(MonarchBalanceCapture {
            account,
            adjusted_balance,
            pending_total,
        })
    }
}

struct MonarchBalanceService<'service> {
    context: &'service AppContext,
    cipher: &'service CredentialCipher,
    provider: &'service dyn MonarchBalanceProvider,
    clock: &'service dyn SyncClock,
}

impl<'service> MonarchBalanceService<'service> {
    fn with_dependencies(
        context: &'service AppContext,
        cipher: &'service CredentialCipher,
        provider: &'service dyn MonarchBalanceProvider,
        clock: &'service dyn SyncClock,
    ) -> Self {
        Self {
            context,
            cipher,
            provider,
            clock,
        }
    }

    async fn run(&self, as_of_date: IsoDate) -> Result<MonarchSyncExecution, MonarchSyncError> {
        let connection_id = self.load_connection().await?;
        let started = self.clock.now();
        let started_at = format_utc_millis(started);
        let stale_cutoff = format_utc_millis(started - STALE_AFTER);
        let claim_connection = self
            .context
            .connection()
            .await
            .map_err(MonarchSyncError::Storage)?;
        let operations = OperationRepository::new(&claim_connection);
        operations
            .interrupt_stale(&started_at, &stale_cutoff)
            .await
            .map_err(MonarchSyncError::from_operation)?;
        let claim = operations
            .claim_manual(MONARCH_CONNECTION_SLUG, &started_at)
            .await
            .map_err(MonarchSyncError::from_operation)?;
        drop(claim_connection);
        let run = match claim {
            ClaimOutcome::Claimed(run) => run,
            ClaimOutcome::AlreadyRunning(run) => {
                return Ok(MonarchSyncExecution::AlreadyRunning(run));
            }
            ClaimOutcome::AlreadyScheduled(_) | ClaimOutcome::CoveredBySuccess(_) => {
                return Err(MonarchSyncError::Coordination);
            }
        };

        match self.capture(connection_id).await {
            Ok(capture) => match self
                .finish_success(&run, connection_id, as_of_date, &capture)
                .await
            {
                Ok(completed) => Ok(MonarchSyncExecution::Completed {
                    run: completed,
                    summary: MonarchBalanceSummary::from_capture(&capture, as_of_date),
                }),
                Err(error) => self.finish_failure(&run, connection_id, error).await,
            },
            Err(error) => self.finish_failure(&run, connection_id, error).await,
        }
    }

    async fn load_connection(&self) -> Result<i64, MonarchSyncError> {
        let connection = self
            .context
            .connection()
            .await
            .map_err(MonarchSyncError::Storage)?;
        let integration = IntegrationRepository::new(&connection)
            .connection_by_slug(MONARCH_CONNECTION_SLUG)
            .await
            .map_err(MonarchSyncError::from_integration)?
            .ok_or(MonarchSyncError::MissingConnection)?;
        if integration.provider != "monarch" {
            return Err(MonarchSyncError::InvalidConnection);
        }
        Ok(integration.id)
    }

    async fn capture(&self, connection_id: i64) -> Result<MonarchBalanceCapture, MonarchSyncError> {
        let connection = self
            .context
            .connection()
            .await
            .map_err(MonarchSyncError::Storage)?;
        let credential = IntegrationRepository::new(&connection)
            .monarch_credential(connection_id)
            .await
            .map_err(MonarchSyncError::from_integration)?
            .ok_or(MonarchSyncError::MissingCredential)?;
        drop(connection);

        if credential.default_account_id.trim().is_empty()
            || credential.default_account_id.len() > MAX_ACCOUNT_ID_BYTES
        {
            return Err(MonarchSyncError::InvalidCredential);
        }
        let access_token = self
            .cipher
            .decrypt_parts(
                &credential.access_token_ciphertext,
                &credential.access_token_nonce,
                credential.key_version,
            )
            .map_err(MonarchSyncError::Credential)?;
        if access_token.trim().is_empty() || access_token.len() > MAX_TOKEN_BYTES {
            return Err(MonarchSyncError::InvalidCredential);
        }
        let capture = self
            .provider
            .capture(access_token.as_str(), &credential.default_account_id)
            .await
            .map_err(MonarchSyncError::Provider)?;
        drop(access_token);
        validate_capture(&capture, &credential.default_account_id)?;
        Ok(capture)
    }

    async fn finish_success(
        &self,
        run: &SyncRun,
        connection_id: i64,
        as_of_date: IsoDate,
        capture: &MonarchBalanceCapture,
    ) -> Result<SyncRun, MonarchSyncError> {
        let persisted_at = format_utc_millis(self.clock.now());
        let connection = self
            .context
            .connection()
            .await
            .map_err(MonarchSyncError::Storage)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(|error| MonarchSyncError::Storage(error.into()))?;
        apply_monarch_balance(
            &transaction,
            connection_id,
            as_of_date,
            capture,
            &persisted_at,
        )
        .await?;
        let completion = SyncCompletion {
            endpoints_hit: Some("account,pendingTransactions".to_owned()),
            ..SyncCompletion::default()
        };
        let completed = OperationRepository::new(&transaction)
            .complete_success(run.id, &persisted_at, &completion)
            .await
            .map_err(MonarchSyncError::from_operation)?;
        transaction
            .commit()
            .await
            .context("commit Monarch balance refresh")
            .map_err(MonarchSyncError::Storage)?;
        Ok(completed)
    }

    async fn finish_failure(
        &self,
        run: &SyncRun,
        connection_id: i64,
        failure: MonarchSyncError,
    ) -> Result<MonarchSyncExecution, MonarchSyncError> {
        let class = failure.failure_class();
        let public_message = failure.public_message();
        tracing::error!(
            sync_run_id = run.id,
            failure_class = class.code(),
            "Monarch balance sync failed"
        );
        let finished_at = format_utc_millis(self.clock.now());
        let connection = self
            .context
            .connection()
            .await
            .map_err(MonarchSyncError::Storage)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(|error| MonarchSyncError::Storage(error.into()))?;
        mark_monarch_error(&transaction, connection_id, public_message, &finished_at).await?;
        let failed = OperationRepository::new(&transaction)
            .complete_error(run.id, &finished_at, public_message)
            .await
            .map_err(MonarchSyncError::from_operation)?;
        transaction
            .commit()
            .await
            .context("commit Monarch balance failure")
            .map_err(MonarchSyncError::Storage)?;
        Ok(MonarchSyncExecution::Failed {
            run: failed,
            class,
            message: public_message,
        })
    }
}

async fn apply_monarch_balance(
    transaction: &Transaction,
    connection_id: i64,
    as_of_date: IsoDate,
    capture: &MonarchBalanceCapture,
    persisted_at: &str,
) -> Result<(), MonarchSyncError> {
    let metadata = json!({
        "reported_balance": capture.account.current_balance,
        "display_balance": capture.account.display_balance,
        "pending_total": capture.pending_total,
        "adjusted_balance": capture.adjusted_balance,
        "account": capture.account.display_name,
        "mask": capture.account.mask,
        "provider_updated_at": capture.account.updated_at,
    });
    let metadata_text = serde_json::to_string(&metadata)
        .context("serialize Monarch balance metadata")
        .map_err(MonarchSyncError::Storage)?;
    if metadata_text.len() > MAX_METADATA_BYTES {
        return Err(MonarchSyncError::InvalidProviderData);
    }
    let account_id = ensure_primary_account(transaction).await?;
    let changed = transaction
        .execute(
            "UPDATE account \
             SET balance = ?2, balance_as_of_date = ?3, source_type = 'monarch', \
                 source_ref = ?4, metadata = ?5, balance_updated_at = ?6, updated_at = ?7 \
             WHERE id = ?1 AND is_primary = 1 AND is_active = 1",
            params![
                account_id,
                capture.adjusted_balance,
                as_of_date.to_string(),
                capture.account.id.clone(),
                metadata_text.clone(),
                capture.account.updated_at.clone(),
                persisted_at,
            ],
        )
        .await
        .context("update primary account from Monarch")
        .map_err(MonarchSyncError::Storage)?;
    if changed != 1 {
        return Err(MonarchSyncError::Coordination);
    }

    transaction
        .execute(
            "INSERT INTO settings (key, value, updated_at) \
             VALUES ('current_cash', ?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![capture.adjusted_balance.to_string(), persisted_at],
        )
        .await
        .context("update current-cash compatibility setting")
        .map_err(MonarchSyncError::Storage)?;
    let balance_source = json!({
        "source_type": "monarch",
        "source_ref": capture.account.id,
        "metadata": metadata,
        "updated_at": capture.account.updated_at,
        "amount": capture.adjusted_balance,
        "as_of_date": as_of_date,
    });
    let balance_source = serde_json::to_string(&balance_source)
        .context("serialize balance-source compatibility setting")
        .map_err(MonarchSyncError::Storage)?;
    if balance_source.len() > MAX_METADATA_BYTES {
        return Err(MonarchSyncError::InvalidProviderData);
    }
    transaction
        .execute(
            "INSERT INTO settings (key, value, updated_at) \
             VALUES ('balance_source', ?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![balance_source, persisted_at],
        )
        .await
        .context("update balance-source compatibility setting")
        .map_err(MonarchSyncError::Storage)?;
    let changed = transaction
        .execute(
            "UPDATE intg_integration_connection \
             SET status = 'active', last_synced_at = ?2, last_error = NULL, updated_at = ?2 \
             WHERE id = ?1 AND slug = 'monarch' AND provider = 'monarch'",
            params![connection_id, persisted_at],
        )
        .await
        .context("update Monarch integration status")
        .map_err(MonarchSyncError::Storage)?;
    if changed != 1 {
        return Err(MonarchSyncError::Coordination);
    }
    Ok(())
}

async fn ensure_primary_account(transaction: &Transaction) -> Result<i64, MonarchSyncError> {
    let mut rows = transaction
        .query(
            "SELECT id FROM account \
             WHERE is_primary = 1 AND is_active = 1 ORDER BY id LIMIT 1",
            (),
        )
        .await
        .context("find primary account for Monarch balance")
        .map_err(MonarchSyncError::Storage)?;
    if let Some(row) = rows
        .next()
        .await
        .context("read primary account for Monarch balance")
        .map_err(MonarchSyncError::Storage)?
    {
        return row
            .get::<i64>(0)
            .context("decode primary account for Monarch balance")
            .map_err(MonarchSyncError::Storage);
    }
    drop(rows);
    let mut rows = transaction
        .query(
            "INSERT INTO account (name, kind, is_primary, is_active) \
             VALUES ('Primary Cash', 'cash', 1, 1) RETURNING id",
            (),
        )
        .await
        .context("insert primary account for Monarch balance")
        .map_err(MonarchSyncError::Storage)?;
    let row = rows
        .next()
        .await
        .context("read inserted primary account for Monarch balance")
        .map_err(MonarchSyncError::Storage)?
        .ok_or(MonarchSyncError::Coordination)?;
    row.get::<i64>(0)
        .context("decode inserted primary account for Monarch balance")
        .map_err(MonarchSyncError::Storage)
}

async fn mark_monarch_error(
    transaction: &Transaction,
    connection_id: i64,
    message: &str,
    updated_at: &str,
) -> Result<(), MonarchSyncError> {
    let changed = transaction
        .execute(
            "UPDATE intg_integration_connection \
             SET status = 'error', last_error = ?2, updated_at = ?3 \
             WHERE id = ?1 AND slug = 'monarch' AND provider = 'monarch'",
            params![connection_id, message, updated_at],
        )
        .await
        .context("mark Monarch integration error")
        .map_err(MonarchSyncError::Storage)?;
    if changed != 1 {
        return Err(MonarchSyncError::Coordination);
    }
    Ok(())
}

fn validate_capture(
    capture: &MonarchBalanceCapture,
    expected_account_id: &str,
) -> Result<(), MonarchSyncError> {
    for value in [
        capture.account.current_balance,
        capture.account.display_balance,
        capture.pending_total,
        capture.adjusted_balance,
    ] {
        if !value.is_finite() || value.abs() >= MAX_ABS_BALANCE {
            return Err(MonarchSyncError::InvalidProviderData);
        }
    }
    validate_bounded_nonempty(&capture.account.id, MAX_ACCOUNT_ID_BYTES)?;
    if capture.account.id != expected_account_id {
        return Err(MonarchSyncError::InvalidProviderData);
    }
    validate_bounded_nonempty(&capture.account.display_name, MAX_ACCOUNT_NAME_BYTES)?;
    if capture
        .account
        .mask
        .as_ref()
        .is_some_and(|value| value.len() > MAX_MASK_BYTES)
    {
        return Err(MonarchSyncError::InvalidProviderData);
    }
    validate_bounded_nonempty(&capture.account.updated_at, MAX_PROVIDER_TIMESTAMP_BYTES)?;
    chrono::DateTime::parse_from_rfc3339(&capture.account.updated_at)
        .map_err(|_| MonarchSyncError::InvalidProviderData)?;
    Ok(())
}

fn validate_bounded_nonempty(value: &str, max_bytes: usize) -> Result<(), MonarchSyncError> {
    if value.trim().is_empty() || value.len() > max_bytes {
        return Err(MonarchSyncError::InvalidProviderData);
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
struct MonarchBalanceSummary {
    reported_balance: f64,
    pending_total: f64,
    adjusted_balance: f64,
    account: String,
    provider_updated_at: String,
    as_of_date: String,
}

impl MonarchBalanceSummary {
    fn from_capture(capture: &MonarchBalanceCapture, as_of_date: IsoDate) -> Self {
        Self {
            reported_balance: capture.account.current_balance,
            pending_total: capture.pending_total,
            adjusted_balance: capture.adjusted_balance,
            account: capture.account.display_name.clone(),
            provider_updated_at: capture.account.updated_at.clone(),
            as_of_date: as_of_date.to_string(),
        }
    }
}

enum MonarchSyncExecution {
    Completed {
        run: SyncRun,
        summary: MonarchBalanceSummary,
    },
    Failed {
        run: SyncRun,
        class: MonarchFailureClass,
        message: &'static str,
    },
    AlreadyRunning(SyncRun),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MonarchFailureClass {
    Configuration,
    Provider,
    Storage,
    Coordination,
}

impl MonarchFailureClass {
    const fn status(self) -> StatusCode {
        match self {
            Self::Configuration | Self::Coordination => StatusCode::CONFLICT,
            Self::Provider => StatusCode::BAD_GATEWAY,
            Self::Storage => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    const fn code(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::Provider => "provider",
            Self::Storage => "service_unavailable",
            Self::Coordination => "coordination",
        }
    }
}

#[derive(Debug)]
enum MonarchSyncError {
    MissingConnection,
    InvalidConnection,
    MissingCredential,
    InvalidCredential,
    Credential(CredentialCryptoError),
    Provider(ProviderError),
    InvalidProviderData,
    Operation(OperationError),
    Storage(anyhow::Error),
    Coordination,
}

impl MonarchSyncError {
    fn from_integration(error: IntegrationError) -> Self {
        match error {
            IntegrationError::Storage(error) => Self::Storage(error),
            IntegrationError::Configuration(_) | IntegrationError::Validation(_) => {
                Self::InvalidConnection
            }
        }
    }

    fn from_operation(error: OperationError) -> Self {
        match error {
            OperationError::ConnectionNotFound(_) => Self::MissingConnection,
            OperationError::Storage(error) => Self::Storage(error),
            error => Self::Operation(error),
        }
    }

    const fn failure_class(&self) -> MonarchFailureClass {
        match self {
            Self::MissingConnection
            | Self::InvalidConnection
            | Self::MissingCredential
            | Self::InvalidCredential
            | Self::Credential(_) => MonarchFailureClass::Configuration,
            Self::Provider(_) | Self::InvalidProviderData => MonarchFailureClass::Provider,
            Self::Storage(_) => MonarchFailureClass::Storage,
            Self::Operation(_) | Self::Coordination => MonarchFailureClass::Coordination,
        }
    }

    const fn public_message(&self) -> &'static str {
        match self {
            Self::MissingConnection | Self::InvalidConnection => {
                "The Monarch integration is not configured."
            }
            Self::MissingCredential => "Monarch credentials are not configured.",
            Self::InvalidCredential | Self::Credential(_) => {
                "Stored Monarch credentials could not be used."
            }
            Self::Provider(_) => "Monarch could not refresh the account balance.",
            Self::InvalidProviderData => {
                "Monarch returned balance data that could not be stored safely."
            }
            Self::Storage(_) => "Balance synchronization is temporarily unavailable.",
            Self::Operation(OperationError::ReadOnly) => "Writes are temporarily disabled.",
            Self::Operation(_) | Self::Coordination => {
                "The balance execution record changed before completion."
            }
        }
    }
}

impl fmt::Display for MonarchSyncError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.public_message())
    }
}

impl Error for MonarchSyncError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Credential(error) => Some(error),
            Self::Provider(error) => Some(error),
            Self::Operation(error) => Some(error),
            Self::Storage(error) => Some(error.as_ref()),
            Self::MissingConnection
            | Self::InvalidConnection
            | Self::MissingCredential
            | Self::InvalidCredential
            | Self::InvalidProviderData
            | Self::Coordination => None,
        }
    }
}

fn monarch_execution_response(execution: MonarchSyncExecution) -> Response {
    match execution {
        MonarchSyncExecution::Completed { run, summary } => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "outcome": "completed",
                "run_id": run.id,
                "reported_balance": summary.reported_balance,
                "pending_total": summary.pending_total,
                "adjusted_balance": summary.adjusted_balance,
                "account": summary.account,
                "updated_at": summary.provider_updated_at,
                "as_of_date": summary.as_of_date,
            })),
        )
            .into_response(),
        MonarchSyncExecution::Failed {
            run,
            class,
            message,
        } => {
            let mut response = (
                class.status(),
                Json(json!({
                    "error": class.code(),
                    "message": message,
                    "outcome": "failed",
                    "run_id": run.id,
                })),
            )
                .into_response();
            if class == MonarchFailureClass::Storage {
                response.headers_mut().insert(
                    header::RETRY_AFTER,
                    HeaderValue::from_static(RETRY_AFTER_SECONDS),
                );
            }
            response
        }
        MonarchSyncExecution::AlreadyRunning(run) => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "already_running",
                "message": "A Monarch balance refresh is already running.",
                "outcome": "already_running",
                "run_id": run.id,
                "started_at": run.started_at,
            })),
        )
            .into_response(),
    }
}

fn monarch_error_response(error: MonarchSyncError) -> Response {
    let class = error.failure_class();
    tracing::error!(
        failure_class = class.code(),
        "Monarch balance request failed"
    );
    let mut response = (
        class.status(),
        Json(json!({
            "error": class.code(),
            "message": error.public_message(),
        })),
    )
        .into_response();
    if class == MonarchFailureClass::Storage {
        response.headers_mut().insert(
            header::RETRY_AFTER,
            HeaderValue::from_static(RETRY_AFTER_SECONDS),
        );
    }
    response
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
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::body::to_bytes;
    use libsql::Builder;
    use serde_json::Value;
    use time::{Date, Month};

    use super::*;
    use crate::{
        crypto::CredentialCipher,
        operations::{OperationRepository, SyncRunStatus},
        providers::{ProviderError, ProviderName},
    };

    const TEST_KEY: &str = "monarch-action-test-key";
    const TOKEN: &str = "fixture-monarch-token";

    #[derive(Clone)]
    struct FixtureProvider {
        result: Result<MonarchBalanceCapture, ProviderError>,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl MonarchBalanceProvider for FixtureProvider {
        async fn capture(
            &self,
            access_token: &str,
            account_id: &str,
        ) -> ProviderResult<MonarchBalanceCapture> {
            assert_eq!(access_token, TOKEN);
            assert_eq!(account_id, "account-123");
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.result.clone()
        }
    }

    #[derive(Clone, Copy)]
    struct FixedClock(OffsetDateTime);

    impl SyncClock for FixedClock {
        fn now(&self) -> OffsetDateTime {
            self.0
        }
    }

    fn clock() -> FixedClock {
        FixedClock(
            Date::from_calendar_date(2026, Month::July, 14)
                .unwrap()
                .with_hms(18, 30, 0)
                .unwrap()
                .assume_utc(),
        )
    }

    fn capture() -> MonarchBalanceCapture {
        MonarchBalanceCapture {
            account: AccountBalance {
                id: "account-123".into(),
                display_name: "Household Checking".into(),
                display_balance: 1_200.0,
                current_balance: 1_200.0,
                updated_at: "2026-07-14T18:29:00Z".into(),
                mask: Some("4321".into()),
            },
            adjusted_balance: 1_150.0,
            pending_total: -50.0,
        }
    }

    async fn context_with_monarch() -> (AppContext, CredentialCipher) {
        let database = Builder::new_local(":memory:").build().await.unwrap();
        let context = AppContext::from_database(database).await.unwrap();
        let cipher = CredentialCipher::new(TEST_KEY).unwrap();
        let encrypted = cipher.encrypt(TOKEN).unwrap();
        let connection = context.connection().await.unwrap();
        connection
            .execute(
                "INSERT INTO intg_integration_connection ( \
                    id, slug, name, provider, status, sync_cadence \
                 ) VALUES (7, 'monarch', 'Monarch', 'monarch', 'error', 'manual')",
                (),
            )
            .await
            .unwrap();
        connection
            .execute(
                "INSERT INTO intg_monarch_credential ( \
                    connection_id, access_token_ciphertext, access_token_nonce, \
                    default_account_id, key_version \
                 ) VALUES (7, ?1, ?2, 'account-123', ?3)",
                params![encrypted.ciphertext, encrypted.nonce, encrypted.key_version,],
            )
            .await
            .unwrap();
        OperationRepository::new(&connection)
            .enable_writes("2026-07-14T18:00:00.000Z")
            .await
            .unwrap();
        (context, cipher)
    }

    #[tokio::test]
    async fn cadence_writes_only_canonical_values_and_recomputes_next_slot() {
        let (context, _) = context_with_monarch().await;
        context
            .connection()
            .await
            .unwrap()
            .execute(
                "INSERT INTO intg_integration_connection ( \
                    id, slug, name, provider, sync_cadence \
                 ) VALUES (8, 'tmo', 'The Mortgage Office', 'mortgage_office', 'manual')",
                (),
            )
            .await
            .unwrap();
        let now = clock().now();
        let next = persist_cadence(&context, "tmo", SyncCadence::Every6h, now)
            .await
            .unwrap();
        assert_eq!(next.as_deref(), Some("2026-07-15T00:00:00.000Z"));
        let connection = context.connection().await.unwrap();
        let integration = IntegrationRepository::new(&connection)
            .connection_by_slug("tmo")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(integration.sync_cadence, "every_6h");
        assert_eq!(integration.next_scheduled_at, next);
        assert!(canonical_cadence("6h").is_none());
        assert!(canonical_cadence(" every_6h ").is_none());
        for (raw, expected) in [
            ("manual", SyncCadence::Manual),
            ("hourly", SyncCadence::Hourly),
            ("every_6h", SyncCadence::Every6h),
            ("every_12h", SyncCadence::Every12h),
            ("daily", SyncCadence::Daily),
        ] {
            assert_eq!(canonical_cadence(raw), Some(expected));
        }

        assert_eq!(
            persist_cadence(&context, "tmo", SyncCadence::Manual, now)
                .await
                .unwrap(),
            None
        );
        let integration = IntegrationRepository::new(&connection)
            .connection_by_slug("tmo")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(integration.sync_cadence, "manual");
        assert!(integration.next_scheduled_at.is_none());
    }

    #[tokio::test]
    async fn htmx_invalid_cadence_gets_a_safe_swappable_fragment() {
        let context = context_with_monarch().await.0;
        let mut headers = HeaderMap::new();
        headers.insert("HX-Request", HeaderValue::from_static("true"));
        let response = update_cadence(
            Extension(context),
            Path("monarch".into()),
            headers,
            Form(CadenceRequest {
                sync_cadence: "6h<script>secret</script>".into(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("Choose manual"));
        assert!(!body.contains("script"));
        assert!(!body.contains("secret"));
    }

    #[tokio::test]
    async fn automatic_cadence_is_not_advertised_for_an_unwired_provider() {
        let context = context_with_monarch().await.0;
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        let response = update_cadence(
            Extension(context.clone()),
            Path("monarch".into()),
            headers,
            Form(CadenceRequest {
                sync_cadence: "every_6h".into(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["error"], "unsupported_integration");
        let connection = context.connection().await.unwrap();
        let integration = IntegrationRepository::new(&connection)
            .connection_by_slug("monarch")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(integration.sync_cadence, "manual");
        assert!(integration.next_scheduled_at.is_none());
    }

    #[tokio::test]
    async fn monarch_success_is_durable_and_updates_cash_and_settings_atomically() {
        let (context, cipher) = context_with_monarch().await;
        let provider = FixtureProvider {
            result: Ok(capture()),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let fixed_clock = clock();
        let service =
            MonarchBalanceService::with_dependencies(&context, &cipher, &provider, &fixed_clock);
        let execution = service.run("2026-07-14".parse().unwrap()).await.unwrap();
        assert!(matches!(execution, MonarchSyncExecution::Completed { .. }));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

        let connection = context.connection().await.unwrap();
        let mut rows = connection
            .query(
                "SELECT balance, balance_as_of_date, source_type, source_ref, metadata, \
                        balance_updated_at FROM account WHERE is_primary = 1",
                (),
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        assert_eq!(row.get::<f64>(0).unwrap(), 1_150.0);
        assert_eq!(row.get::<String>(1).unwrap(), "2026-07-14");
        assert_eq!(row.get::<String>(2).unwrap(), "monarch");
        assert_eq!(row.get::<String>(3).unwrap(), "account-123");
        let metadata: Value = serde_json::from_str(&row.get::<String>(4).unwrap()).unwrap();
        assert_eq!(metadata["pending_total"], -50.0);
        assert_eq!(row.get::<String>(5).unwrap(), "2026-07-14T18:29:00Z");
        drop(rows);

        let repository = IntegrationRepository::new(&connection);
        assert_eq!(
            repository
                .setting("current_cash")
                .await
                .unwrap()
                .unwrap()
                .value,
            "1150"
        );
        let balance_source: Value = serde_json::from_str(
            &repository
                .setting("balance_source")
                .await
                .unwrap()
                .unwrap()
                .value,
        )
        .unwrap();
        assert_eq!(balance_source["as_of_date"], "2026-07-14");
        assert_eq!(balance_source["metadata"]["reported_balance"], 1_200.0);
        let integration = repository
            .connection_by_slug("monarch")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(integration.status, "active");
        assert!(integration.last_error.is_none());
        let runs = OperationRepository::new(&connection)
            .list_recent("monarch", 10)
            .await
            .unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, SyncRunStatus::Success);
        assert_eq!(
            runs[0].endpoints_hit.as_deref(),
            Some("account,pendingTransactions")
        );
    }

    #[tokio::test]
    async fn provider_failure_is_sanitized_recorded_and_does_not_change_cash() {
        let (context, cipher) = context_with_monarch().await;
        let provider = FixtureProvider {
            result: Err(ProviderError::HttpStatus {
                provider: ProviderName::Monarch,
                status: 418,
            }),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let fixed_clock = clock();
        let service =
            MonarchBalanceService::with_dependencies(&context, &cipher, &provider, &fixed_clock);
        let execution = service.run("2026-07-14".parse().unwrap()).await.unwrap();
        let MonarchSyncExecution::Failed {
            run,
            class,
            message,
        } = execution
        else {
            panic!("expected a durable failure");
        };
        assert_eq!(class, MonarchFailureClass::Provider);
        assert_eq!(message, "Monarch could not refresh the account balance.");
        assert!(!message.contains("418"));
        assert_eq!(run.status, SyncRunStatus::Error);
        assert_eq!(run.error_message.as_deref(), Some(message));
        let connection = context.connection().await.unwrap();
        let mut rows = connection
            .query("SELECT COUNT(*) FROM account WHERE balance IS NOT NULL", ())
            .await
            .unwrap();
        assert_eq!(
            rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
            0
        );
        let integration = IntegrationRepository::new(&connection)
            .connection_by_slug("monarch")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(integration.status, "error");
        assert_eq!(integration.last_error.as_deref(), Some(message));
    }

    #[tokio::test]
    async fn failed_completion_rolls_back_account_and_compatibility_settings_together() {
        let (context, _) = context_with_monarch().await;
        let connection = context.connection().await.unwrap();
        connection
            .execute(
                "DELETE FROM intg_integration_connection WHERE slug = 'monarch'",
                (),
            )
            .await
            .unwrap();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .unwrap();
        let result = apply_monarch_balance(
            &transaction,
            7,
            "2026-07-14".parse().unwrap(),
            &capture(),
            "2026-07-14T18:30:00.000Z",
        )
        .await;
        assert!(matches!(result, Err(MonarchSyncError::Coordination)));
        transaction.rollback().await.unwrap();

        let mut rows = connection
            .query("SELECT COUNT(*) FROM account", ())
            .await
            .unwrap();
        assert_eq!(
            rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
            0
        );
        drop(rows);
        let mut rows = connection
            .query(
                "SELECT COUNT(*) FROM settings \
                 WHERE key IN ('current_cash', 'balance_source')",
                (),
            )
            .await
            .unwrap();
        assert_eq!(
            rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn running_claim_prevents_a_duplicate_provider_request() {
        let (context, cipher) = context_with_monarch().await;
        let connection = context.connection().await.unwrap();
        OperationRepository::new(&connection)
            .claim_manual("monarch", "2026-07-14T18:29:59.000Z")
            .await
            .unwrap();
        let provider = FixtureProvider {
            result: Ok(capture()),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let fixed_clock = clock();
        let service =
            MonarchBalanceService::with_dependencies(&context, &cipher, &provider, &fixed_clock);
        let execution = service.run("2026-07-14".parse().unwrap()).await.unwrap();
        assert!(matches!(execution, MonarchSyncExecution::AlreadyRunning(_)));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn non_finite_provider_values_are_rejected_before_any_cash_write() {
        let (context, cipher) = context_with_monarch().await;
        let mut invalid = capture();
        invalid.adjusted_balance = f64::INFINITY;
        let provider = FixtureProvider {
            result: Ok(invalid),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let fixed_clock = clock();
        let service =
            MonarchBalanceService::with_dependencies(&context, &cipher, &provider, &fixed_clock);
        let execution = service.run("2026-07-14".parse().unwrap()).await.unwrap();
        assert!(matches!(
            execution,
            MonarchSyncExecution::Failed {
                class: MonarchFailureClass::Provider,
                ..
            }
        ));
        let connection = context.connection().await.unwrap();
        let mut rows = connection
            .query("SELECT COUNT(*) FROM account WHERE balance IS NOT NULL", ())
            .await
            .unwrap();
        assert_eq!(
            rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn invalid_browser_date_is_rejected_before_database_or_provider_work() {
        let database = Builder::new_local(":memory:").build().await.unwrap();
        let context = AppContext::from_database(database).await.unwrap();
        let response = sync_monarch_balance(
            Extension(context),
            Extension(Arc::new(CredentialCipher::new(TEST_KEY).unwrap())),
            Json(MonarchBalanceRequest {
                as_of_date: "2026-02-31".into(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8_lossy(&body);
        assert!(!body.contains("2026-02-31"));
        assert!(body.contains("valid YYYY-MM-DD"));
    }
}
