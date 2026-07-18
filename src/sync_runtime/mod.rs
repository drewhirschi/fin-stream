use std::{error::Error, fmt};

use async_trait::async_trait;
use libsql::TransactionBehavior;
use time::{Duration, OffsetDateTime};

use crate::{
    crypto::{CredentialCipher, CredentialCryptoError},
    db::AppContext,
    integrations::{
        IntegrationError, IntegrationRepository, IntegrationWriteRepository, TmoSyncCapture,
    },
    operations::{
        ClaimOutcome, OperationError, OperationRepository, SyncCompletion, SyncRun, SyncRunStatus,
    },
    providers::{
        ProviderError, ProviderResult,
        tmo::{
            TmoClient, TmoCredentials, TmoLoanDetail, TmoLoanSummary, TmoOverview, TmoPayment,
            TmoUserInfo,
        },
    },
};

pub mod http;

pub const TMO_CONNECTION_SLUG: &str = "tmo";

/// Must remain greater than the deployed function's hard execution limit plus
/// clock/network skew. Until Vercel `maxDuration` is measured and pinned below
/// this value, the runtime uses a conservative twenty-minute stale boundary.
/// Production must keep the platform limit plus skew below this cutoff.
const STALE_AFTER: Duration = Duration::minutes(20);

#[async_trait]
pub trait TmoProviderSession: Send + Sync {
    fn user(&self) -> &TmoUserInfo;
    async fn overview(&self) -> ProviderResult<TmoOverview>;
    async fn portfolio(&self) -> ProviderResult<Vec<TmoLoanSummary>>;
    async fn loan_detail(&self, loan_account: &str) -> ProviderResult<TmoLoanDetail>;
    async fn history(&self) -> ProviderResult<Vec<TmoPayment>>;
}

#[async_trait]
pub trait TmoProviderFactory: Send + Sync {
    async fn login(
        &self,
        credentials: TmoCredentials,
    ) -> ProviderResult<Box<dyn TmoProviderSession>>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DirectTmoProviderFactory;

#[async_trait]
impl TmoProviderFactory for DirectTmoProviderFactory {
    async fn login(
        &self,
        credentials: TmoCredentials,
    ) -> ProviderResult<Box<dyn TmoProviderSession>> {
        let client = TmoClient::login(&credentials).await?;
        Ok(Box::new(client))
    }
}

#[async_trait]
impl TmoProviderSession for TmoClient {
    fn user(&self) -> &TmoUserInfo {
        TmoClient::user(self)
    }

    async fn overview(&self) -> ProviderResult<TmoOverview> {
        self.get_overview().await
    }

    async fn portfolio(&self) -> ProviderResult<Vec<TmoLoanSummary>> {
        self.get_portfolio().await
    }

    async fn loan_detail(&self, loan_account: &str) -> ProviderResult<TmoLoanDetail> {
        self.get_loan_detail(loan_account).await
    }

    async fn history(&self) -> ProviderResult<Vec<TmoPayment>> {
        self.get_history(None).await
    }
}

pub trait SyncClock: Send + Sync {
    fn now(&self) -> OffsetDateTime;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemSyncClock;

impl SyncClock for SystemSyncClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

pub struct TmoSyncService<'service> {
    context: &'service AppContext,
    cipher: &'service CredentialCipher,
    provider_factory: &'service dyn TmoProviderFactory,
    clock: &'service dyn SyncClock,
}

impl<'service> TmoSyncService<'service> {
    pub fn production(
        context: &'service AppContext,
        cipher: &'service CredentialCipher,
        provider_factory: &'service DirectTmoProviderFactory,
        clock: &'service SystemSyncClock,
    ) -> Self {
        Self {
            context,
            cipher,
            provider_factory,
            clock,
        }
    }

    pub fn with_dependencies(
        context: &'service AppContext,
        cipher: &'service CredentialCipher,
        provider_factory: &'service dyn TmoProviderFactory,
        clock: &'service dyn SyncClock,
    ) -> Self {
        Self {
            context,
            cipher,
            provider_factory,
            clock,
        }
    }

    pub async fn run_manual(&self) -> Result<SyncExecution, SyncRuntimeError> {
        self.run(SyncTrigger::Manual).await
    }

    /// Cron-compatible entry point. The caller supplies the deterministic UTC
    /// slot and must already be behind the scheduler-enabled write gate.
    pub async fn run_scheduled(
        &self,
        scheduled_for: &str,
    ) -> Result<SyncExecution, SyncRuntimeError> {
        self.run(SyncTrigger::Scheduled(scheduled_for.to_owned()))
            .await
    }

    async fn run(&self, trigger: SyncTrigger) -> Result<SyncExecution, SyncRuntimeError> {
        let started = self.clock.now();
        let started_at = format_utc_millis(started);
        let stale_cutoff = format_utc_millis(started - STALE_AFTER);
        let claim_connection = self
            .context
            .connection()
            .await
            .map_err(SyncRuntimeError::Storage)?;
        let operations = OperationRepository::new(&claim_connection);

        // Vercel may terminate an invocation before its error transition can
        // run. Clear only executions older than the hard stale boundary before
        // attempting the unique-index-backed claim.
        operations
            .interrupt_stale(&started_at, &stale_cutoff)
            .await
            .map_err(SyncRuntimeError::Operation)?;
        let outcome = match trigger {
            SyncTrigger::Manual => {
                operations
                    .claim_manual(TMO_CONNECTION_SLUG, &started_at)
                    .await
            }
            SyncTrigger::Scheduled(slot) => {
                operations
                    .claim_scheduled(TMO_CONNECTION_SLUG, &slot, &started_at)
                    .await
            }
        }
        .map_err(SyncRuntimeError::Operation)?;
        drop(claim_connection);

        let run = match outcome {
            ClaimOutcome::Claimed(run) => run,
            ClaimOutcome::AlreadyRunning(run) => {
                return Ok(SyncExecution::AlreadyRunning(run));
            }
            ClaimOutcome::AlreadyScheduled(run) => {
                return Ok(SyncExecution::AlreadyScheduled(run));
            }
            ClaimOutcome::CoveredBySuccess(run) => {
                return Ok(SyncExecution::CoveredBySuccess(run));
            }
        };

        let connection_result = self.load_tmo_connection().await;
        let (connection_id, capture_result) = match connection_result {
            Ok(connection_id) => (Some(connection_id), self.capture(connection_id).await),
            Err(error) => (None, Err(error)),
        };
        match capture_result {
            Ok(capture) => match self.finish_success(&run, &capture).await {
                Ok(completed) => Ok(SyncExecution::Completed(completed)),
                Err(error) => self.finish_failure(&run, connection_id, error).await,
            },
            Err(error) => self.finish_failure(&run, connection_id, error).await,
        }
    }

    async fn load_tmo_connection(&self) -> Result<i64, SyncRuntimeError> {
        let connection = self
            .context
            .connection()
            .await
            .map_err(SyncRuntimeError::Storage)?;
        let value = IntegrationRepository::new(&connection)
            .connection_by_slug(TMO_CONNECTION_SLUG)
            .await
            .map_err(SyncRuntimeError::from_integration)?
            .ok_or(SyncRuntimeError::MissingConnection)?;
        Ok(value.id)
    }

    async fn capture(&self, connection_id: i64) -> Result<TmoSyncCapture, SyncRuntimeError> {
        let connection = self
            .context
            .connection()
            .await
            .map_err(SyncRuntimeError::Storage)?;
        let credential = IntegrationRepository::new(&connection)
            .tmo_credential(connection_id)
            .await
            .map_err(SyncRuntimeError::from_integration)?
            .ok_or(SyncRuntimeError::MissingCredential)?;
        drop(connection);

        let pin = self
            .cipher
            .decrypt_parts(
                &credential.pin_ciphertext,
                &credential.pin_nonce,
                credential.key_version,
            )
            .map_err(SyncRuntimeError::Credential)?;
        let credentials = TmoCredentials::new(
            credential.company_id.clone(),
            credential.account_number.clone(),
            pin.as_str(),
        );
        drop(pin);
        let provider = self
            .provider_factory
            .login(credentials)
            .await
            .map_err(SyncRuntimeError::Provider)?;

        let user = provider.user().clone();
        let overview = provider
            .overview()
            .await
            .map_err(SyncRuntimeError::Provider)?;
        let loans = provider
            .portfolio()
            .await
            .map_err(SyncRuntimeError::Provider)?;
        let mut loan_details = Vec::with_capacity(loans.len());
        let mut loan_detail_failures = 0_i64;
        for loan in &loans {
            match provider.loan_detail(&loan.loan_account).await {
                Ok(detail) => loan_details.push(detail),
                Err(_) => {
                    loan_detail_failures =
                        loan_detail_failures.checked_add(1).ok_or_else(|| {
                            SyncRuntimeError::InvalidCapture(
                                "loan-detail failure counter overflowed".to_owned(),
                            )
                        })?;
                }
            }
        }
        let payments = provider
            .history()
            .await
            .map_err(SyncRuntimeError::Provider)?;
        let captured = self.clock.now();

        Ok(TmoSyncCapture {
            connection_id,
            captured_at: format_utc_millis(captured),
            snapshot_date: format_date(captured),
            company_id: credential.company_id,
            account_number: credential.account_number,
            user,
            overview,
            loans,
            loan_details,
            loan_detail_failures,
            payments,
        })
    }

    async fn finish_success(
        &self,
        run: &SyncRun,
        capture: &TmoSyncCapture,
    ) -> Result<SyncRun, SyncRuntimeError> {
        let connection = self
            .context
            .connection()
            .await
            .map_err(SyncRuntimeError::Storage)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(|error| SyncRuntimeError::Storage(error.into()))?;
        let persistence = IntegrationWriteRepository::new(&transaction)
            .apply_tmo_sync(capture)
            .await
            .map_err(SyncRuntimeError::from_integration)?;
        let detail_attempts = capture.loan_details.len() + capture.loan_detail_failures as usize;
        let endpoints_hit = if capture.loan_detail_failures == 0 {
            "overview,portfolio,loanDetail,history".to_owned()
        } else {
            format!(
                "overview,portfolio,loanDetail:{}/{detail_attempts},history",
                capture.loan_details.len()
            )
        };
        let completion = SyncCompletion {
            endpoints_hit: Some(endpoints_hit),
            events_upserted: persistence.events_upserted,
            loans_upserted: persistence.loans_upserted,
            snapshots_created: persistence.snapshots_created,
        };
        let finished_at = format_utc_millis(self.clock.now());
        let completed = OperationRepository::new(&transaction)
            .complete_success(run.id, &finished_at, &completion)
            .await
            .map_err(SyncRuntimeError::Operation)?;
        transaction
            .commit()
            .await
            .map_err(|error| SyncRuntimeError::Storage(error.into()))?;
        Ok(completed)
    }

    async fn finish_failure(
        &self,
        run: &SyncRun,
        connection_id: Option<i64>,
        failure: SyncRuntimeError,
    ) -> Result<SyncExecution, SyncRuntimeError> {
        let public_message = failure.public_message();
        tracing::error!(
            sync_run_id = run.id,
            failure_class = failure.class_code(),
            "TMO sync failed"
        );
        let connection = self
            .context
            .connection()
            .await
            .map_err(SyncRuntimeError::Storage)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(|error| SyncRuntimeError::Storage(error.into()))?;
        if let Some(connection_id) = connection_id {
            IntegrationWriteRepository::new(&transaction)
                .mark_tmo_error(
                    connection_id,
                    public_message,
                    &format_utc_millis(self.clock.now()),
                )
                .await
                .map_err(SyncRuntimeError::from_integration)?;
        }
        let failed = OperationRepository::new(&transaction)
            .complete_error(run.id, &format_utc_millis(self.clock.now()), public_message)
            .await
            .map_err(SyncRuntimeError::Operation)?;
        transaction
            .commit()
            .await
            .map_err(|error| SyncRuntimeError::Storage(error.into()))?;
        Ok(SyncExecution::Failed {
            run: failed,
            class: failure.failure_class(),
        })
    }
}

enum SyncTrigger {
    Manual,
    Scheduled(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncFailureClass {
    Configuration,
    Provider,
    Storage,
    Coordination,
}

impl SyncFailureClass {
    pub const fn http_status(self) -> axum::http::StatusCode {
        match self {
            Self::Configuration => axum::http::StatusCode::CONFLICT,
            Self::Provider => axum::http::StatusCode::BAD_GATEWAY,
            Self::Storage => axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Self::Coordination => axum::http::StatusCode::CONFLICT,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyncExecution {
    Completed(SyncRun),
    Failed {
        run: SyncRun,
        class: SyncFailureClass,
    },
    AlreadyRunning(SyncRun),
    AlreadyScheduled(SyncRun),
    CoveredBySuccess(SyncRun),
}

impl SyncExecution {
    pub fn run(&self) -> &SyncRun {
        match self {
            Self::Completed(run)
            | Self::Failed { run, .. }
            | Self::AlreadyRunning(run)
            | Self::AlreadyScheduled(run)
            | Self::CoveredBySuccess(run) => run,
        }
    }

    pub const fn outcome_code(&self) -> &'static str {
        match self {
            Self::Completed(_) => "completed",
            Self::Failed { .. } => "failed",
            Self::AlreadyRunning(_) => "already_running",
            Self::AlreadyScheduled(_) => "already_scheduled",
            Self::CoveredBySuccess(_) => "covered_by_success",
        }
    }

    pub const fn http_status(&self) -> axum::http::StatusCode {
        match self {
            Self::Completed(_) | Self::CoveredBySuccess(_) => axum::http::StatusCode::OK,
            Self::Failed { class, .. } => class.http_status(),
            Self::AlreadyRunning(_) | Self::AlreadyScheduled(_) => axum::http::StatusCode::CONFLICT,
        }
    }
}

#[derive(Debug)]
pub enum SyncRuntimeError {
    MissingConnection,
    MissingCredential,
    Configuration(String),
    Credential(CredentialCryptoError),
    Provider(ProviderError),
    InvalidCapture(String),
    Integration(IntegrationError),
    Operation(OperationError),
    Storage(anyhow::Error),
}

impl SyncRuntimeError {
    fn from_integration(error: IntegrationError) -> Self {
        match error {
            IntegrationError::Configuration(message) => Self::Configuration(message),
            IntegrationError::Validation(message) => Self::InvalidCapture(message),
            error @ IntegrationError::Storage(_) => Self::Integration(error),
        }
    }

    pub const fn public_message(&self) -> &'static str {
        match self {
            Self::MissingConnection => "The TMO integration is not configured.",
            Self::MissingCredential => "TMO credentials are not configured.",
            Self::Configuration(_) => "TMO synchronization prerequisites are not configured.",
            Self::Credential(_) => "Stored TMO credentials could not be decrypted.",
            Self::Provider(error) => match error {
                ProviderError::AuthenticationRejected { .. } => "TMO rejected authentication.",
                ProviderError::Timeout { .. } => "A TMO request timed out.",
                _ => "TMO could not complete the synchronization request.",
            },
            Self::Integration(IntegrationError::Validation(_)) | Self::InvalidCapture(_) => {
                "TMO returned data that could not be imported safely."
            }
            Self::Integration(IntegrationError::Configuration(_)) => {
                "TMO synchronization prerequisites are not configured."
            }
            Self::Integration(IntegrationError::Storage(_)) | Self::Storage(_) => {
                "The TMO synchronization database is temporarily unavailable."
            }
            Self::Operation(OperationError::ReadOnly) => "Writes are temporarily disabled.",
            Self::Operation(OperationError::SchedulerDisabled) => {
                "Scheduled synchronization is disabled."
            }
            Self::Operation(_) => "The TMO execution record changed before completion.",
        }
    }

    pub const fn failure_class(&self) -> SyncFailureClass {
        match self {
            Self::MissingConnection
            | Self::MissingCredential
            | Self::Configuration(_)
            | Self::Credential(_)
            | Self::Integration(IntegrationError::Configuration(_)) => {
                SyncFailureClass::Configuration
            }
            Self::Provider(_)
            | Self::InvalidCapture(_)
            | Self::Integration(IntegrationError::Validation(_)) => SyncFailureClass::Provider,
            Self::Integration(IntegrationError::Storage(_)) | Self::Storage(_) => {
                SyncFailureClass::Storage
            }
            Self::Operation(_) => SyncFailureClass::Coordination,
        }
    }

    pub const fn class_code(&self) -> &'static str {
        match self.failure_class() {
            SyncFailureClass::Configuration => "configuration",
            SyncFailureClass::Provider => "provider",
            SyncFailureClass::Storage => "storage",
            SyncFailureClass::Coordination => "coordination",
        }
    }

    pub const fn http_status(&self) -> axum::http::StatusCode {
        self.failure_class().http_status()
    }
}

impl fmt::Display for SyncRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.public_message())
    }
}

impl Error for SyncRuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Credential(error) => Some(error),
            Self::Provider(error) => Some(error),
            Self::Integration(error) => Some(error),
            Self::Operation(error) => Some(error),
            Self::Storage(error) => Some(error.as_ref()),
            Self::MissingConnection
            | Self::MissingCredential
            | Self::Configuration(_)
            | Self::InvalidCapture(_) => None,
        }
    }
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

fn format_date(value: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        value.year(),
        value.month() as u8,
        value.day()
    )
}

pub fn current_or_last(runs: &[SyncRun]) -> Option<SyncRun> {
    runs.iter()
        .find(|run| run.status == SyncRunStatus::Running)
        .or_else(|| runs.first())
        .cloned()
}

#[cfg(all(test, feature = "local-db"))]
mod tests;
