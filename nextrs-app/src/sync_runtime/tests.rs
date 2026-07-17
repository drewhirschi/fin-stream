use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use axum::http::StatusCode;
use libsql::{Builder, Connection, params};
use time::{Date, Month, OffsetDateTime};

use crate::{
    crypto::CredentialCipher,
    db::AppContext,
    finance::FinanceRepository,
    operations::{OperationRepository, SyncRunStatus},
    providers::{
        ProviderError, ProviderName, ProviderResult,
        tmo::{
            TmoCredentials, TmoLoanDetail, TmoLoanSummary, TmoOverview, TmoPayment, TmoUserInfo,
        },
    },
};

use super::{
    SyncClock, SyncExecution, SyncFailureClass, SyncRuntimeError, TMO_CONNECTION_SLUG,
    TmoProviderFactory, TmoProviderSession, TmoSyncService, format_utc_millis,
};

const TEST_KEY: &str = "sync-runtime-test-key";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FixtureMode {
    Success,
    FailOverview,
    PartialDetail,
    InvalidDate,
    CompletionRace,
}

#[derive(Clone)]
struct FixtureFactory {
    mode: FixtureMode,
    login_calls: Arc<AtomicUsize>,
    race_context: Option<AppContext>,
}

impl FixtureFactory {
    fn new(mode: FixtureMode) -> Self {
        Self {
            mode,
            login_calls: Arc::new(AtomicUsize::new(0)),
            race_context: None,
        }
    }

    fn with_race_context(mut self, context: AppContext) -> Self {
        self.race_context = Some(context);
        self
    }
}

#[async_trait]
impl TmoProviderFactory for FixtureFactory {
    async fn login(
        &self,
        _credentials: TmoCredentials,
    ) -> ProviderResult<Box<dyn TmoProviderSession>> {
        self.login_calls.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FixtureSession {
            mode: self.mode,
            race_context: self.race_context.clone(),
        }))
    }
}

struct FixtureSession {
    mode: FixtureMode,
    race_context: Option<AppContext>,
}

#[async_trait]
impl TmoProviderSession for FixtureSession {
    fn user(&self) -> &TmoUserInfo {
        static USER: std::sync::LazyLock<TmoUserInfo> = std::sync::LazyLock::new(|| TmoUserInfo {
            source_rec_id: "source-user-1".into(),
            company_id: "vci".into(),
            account: "fixture-account".into(),
            name: "Fixture User".into(),
            email: "fixture@example.com".into(),
        });
        &USER
    }

    async fn overview(&self) -> ProviderResult<TmoOverview> {
        if self.mode == FixtureMode::FailOverview {
            return Err(ProviderError::RequestRejected {
                provider: ProviderName::Tmo,
            });
        }
        Ok(TmoOverview {
            portfolio_value: 510_000.0,
            portfolio_yield: 8.25,
            ytd_interest: 19_250.5,
            ytd_principal: 7_000.0,
            portfolio_count: 2,
            trust_balance: 42_500.0,
            outstanding_checks_value: 1_200.0,
            ytd_serv_fees: 875.0,
        })
    }

    async fn portfolio(&self) -> ProviderResult<Vec<TmoLoanSummary>> {
        let mut first = loan_summary("LN-100", "Borrower One");
        if self.mode == FixtureMode::InvalidDate {
            first.maturity_date = "not-a-date".into();
        }
        let loans = if self.mode == FixtureMode::PartialDetail {
            vec![first, loan_summary("LN-FAIL", "Borrower Two")]
        } else {
            vec![first]
        };
        Ok(loans)
    }

    async fn loan_detail(&self, loan_account: &str) -> ProviderResult<TmoLoanDetail> {
        if self.mode == FixtureMode::PartialDetail && loan_account == "LN-FAIL" {
            return Err(ProviderError::Timeout {
                provider: ProviderName::Tmo,
            });
        }
        Ok(loan_detail(loan_account))
    }

    async fn history(&self) -> ProviderResult<Vec<TmoPayment>> {
        if self.mode == FixtureMode::CompletionRace {
            let context = self.race_context.as_ref().expect("race context");
            let connection = context.connection().await.unwrap();
            let operations = OperationRepository::new(&connection);
            let run = operations
                .current_run(TMO_CONNECTION_SLUG)
                .await
                .unwrap()
                .expect("claimed run");
            operations
                .complete_error(
                    run.id,
                    "2026-07-14T12:00:01.000Z",
                    "simulated competing completion",
                )
                .await
                .unwrap();
        }
        Ok(vec![TmoPayment {
            check_number: "Print Check".into(),
            loan_account: "LN-100".into(),
            check_date: "2026-07-10T07:00:00.000Z".into(),
            amount: 1_250.25,
            service_fee: 25.0,
            interest: 1_000.25,
            principal: 225.0,
            charges: 0.0,
            late_charges: 0.0,
            other: 0.0,
            borrower_name: "Borrower One".into(),
            property_name: "100 Main St".into(),
        }])
    }
}

#[derive(Clone, Copy)]
struct FixedClock(OffsetDateTime);

impl SyncClock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        self.0
    }
}

fn fixed_clock() -> FixedClock {
    let date = Date::from_calendar_date(2026, Month::July, 14).unwrap();
    FixedClock(date.with_hms(12, 0, 0).unwrap().assume_utc())
}

fn loan_summary(account: &str, borrower: &str) -> TmoLoanSummary {
    TmoLoanSummary {
        loan_account: account.into(),
        borrower_name: borrower.into(),
        primary_street: "100 Main St".into(),
        primary_city: "Denver".into(),
        primary_state: "CO".into(),
        primary_zip: "80202".into(),
        percent_owned: 50.0,
        interest_rate: 8.25,
        maturity_date: "2030-12-31T07:00:00.000Z".into(),
        term_left: 53,
        next_payment_date: "2026-08-01T07:00:00.000Z".into(),
        interest_paid_to_date: "2026-07-01T07:00:00.000Z".into(),
        billed_through: Some("2026-07-31T07:00:00.000Z".into()),
        regular_payment: 1_250.25,
        loan_balance: 115_000.0,
        is_delinquent: false,
    }
}

fn loan_detail(account: &str) -> TmoLoanDetail {
    TmoLoanDetail {
        loan_account: account.into(),
        borrower_name: "Borrower One".into(),
        primary_street: "100 Main St".into(),
        primary_city: "Denver".into(),
        primary_state: "CO".into(),
        primary_zip: "80202".into(),
        property_description: Some("Single family".into()),
        property_type: Some("Residential".into()),
        property_priority: Some(1),
        occupancy: Some("Owner".into()),
        ltv: Some(62.5),
        appraised_value: Some(250_000.0),
        priority: Some(1),
        original_balance: 150_000.0,
        principal_balance: 115_000.0,
        note_rate: 8.25,
        maturity_date: "2030-12-31T07:00:00.000Z".into(),
        next_payment_date: "2026-08-01T07:00:00.000Z".into(),
        interest_paid_to_date: "2026-07-01T07:00:00.000Z".into(),
        regular_payment: 1_250.25,
        payment_frequency: "Monthly".into(),
        loan_type: 1,
    }
}

async fn test_context(with_bootstrap: bool) -> (AppContext, CredentialCipher, i64) {
    let database = Builder::new_local(":memory:").build().await.unwrap();
    let context = AppContext::from_database(database).await.unwrap();
    let connection = context.connection().await.unwrap();
    if with_bootstrap {
        FinanceRepository::new(&connection)
            .bootstrap_defaults("2026-07-14".parse().unwrap())
            .await
            .unwrap();
    }
    OperationRepository::new(&connection)
        .enable_writes("2026-07-14T11:59:00.000Z")
        .await
        .unwrap();
    let mut rows = connection
        .query(
            "INSERT INTO intg_integration_connection (slug, name, provider) \
             VALUES ('tmo', 'The Mortgage Office', 'mortgage_office') RETURNING id",
            (),
        )
        .await
        .unwrap();
    let connection_id = rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap();
    drop(rows);
    let cipher = CredentialCipher::new(TEST_KEY).unwrap();
    let encrypted = cipher.encrypt("fixture-pin").unwrap();
    connection
        .execute(
            "INSERT INTO intg_tmo_credential ( \
                connection_id, company_id, account_number, pin_ciphertext, pin_nonce, key_version \
             ) VALUES (?1, 'vci', 'fixture-account', ?2, ?3, ?4)",
            params![
                connection_id,
                encrypted.ciphertext,
                encrypted.nonce,
                encrypted.key_version,
            ],
        )
        .await
        .unwrap();
    (context, cipher, connection_id)
}

fn service<'a>(
    context: &'a AppContext,
    cipher: &'a CredentialCipher,
    factory: &'a FixtureFactory,
    clock: &'a FixedClock,
) -> TmoSyncService<'a> {
    TmoSyncService::with_dependencies(context, cipher, factory, clock)
}

async fn scalar_i64(connection: &Connection, sql: &str) -> i64 {
    let mut rows = connection.query(sql, ()).await.unwrap();
    rows.next().await.unwrap().unwrap().get(0).unwrap()
}

async fn pair_ids(connection: &Connection) -> (i64, i64) {
    let mut rows = connection
        .query(
            "SELECT payment.id, event.id \
             FROM intg_tmo_import_payment payment \
             JOIN intg_tmo_payment_event_link link ON link.tmo_payment_id = payment.id \
             JOIN stream_event event ON event.id = link.stream_event_id",
            (),
        )
        .await
        .unwrap();
    let row = rows.next().await.unwrap().unwrap();
    (row.get(0).unwrap(), row.get(1).unwrap())
}

#[tokio::test]
async fn successful_sync_is_atomic_and_idempotent() {
    let (context, cipher, _) = test_context(true).await;
    let factory = FixtureFactory::new(FixtureMode::Success);
    let clock = fixed_clock();

    let first = service(&context, &cipher, &factory, &clock)
        .run_manual()
        .await
        .unwrap();
    let first_run = match first {
        SyncExecution::Completed(run) => run,
        outcome => panic!("unexpected outcome: {outcome:?}"),
    };
    assert_eq!(first_run.status, SyncRunStatus::Success);
    assert_eq!(first_run.loans_upserted, 1);
    assert_eq!(first_run.events_upserted, 1);
    assert_eq!(first_run.snapshots_created, 1);
    assert_eq!(
        first_run.endpoints_hit.as_deref(),
        Some("overview,portfolio,loanDetail,history")
    );

    let connection = context.connection().await.unwrap();
    assert_eq!(
        scalar_i64(&connection, "SELECT COUNT(*) FROM portfolio_snapshot").await,
        1
    );
    assert_eq!(
        scalar_i64(&connection, "SELECT COUNT(*) FROM intg_tmo_import_overview").await,
        1
    );
    assert_eq!(
        scalar_i64(&connection, "SELECT COUNT(*) FROM intg_tmo_import_loan").await,
        1
    );
    assert_eq!(
        scalar_i64(&connection, "SELECT COUNT(*) FROM intg_tmo_import_payment").await,
        1
    );
    assert_eq!(
        scalar_i64(
            &connection,
            "SELECT COUNT(*) FROM stream_event WHERE source_type = 'tmo_history'"
        )
        .await,
        1
    );
    assert_eq!(
        scalar_i64(
            &connection,
            "SELECT COUNT(*) FROM intg_tmo_payment_event_link"
        )
        .await,
        1
    );
    let original_ids = pair_ids(&connection).await;

    let second = service(&context, &cipher, &factory, &clock)
        .run_manual()
        .await
        .unwrap();
    assert!(matches!(second, SyncExecution::Completed(_)));
    assert_eq!(pair_ids(&connection).await, original_ids);
    assert_eq!(
        scalar_i64(&connection, "SELECT COUNT(*) FROM portfolio_snapshot").await,
        1
    );
    assert_eq!(
        scalar_i64(
            &connection,
            "SELECT COUNT(*) FROM sync_log WHERE status = 'success'"
        )
        .await,
        2
    );

    let mut rows = connection
        .query(
            "SELECT processing_state, normalized_event_source_id, raw_payload \
             FROM intg_tmo_import_payment",
            (),
        )
        .await
        .unwrap();
    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<String>(0).unwrap(), "normalized");
    assert_eq!(
        row.get::<String>(1).unwrap(),
        "history:LN-100:2026-07-10:125025"
    );
    assert!(row.get::<String>(2).unwrap().contains("Borrower One"));
}

#[tokio::test]
async fn fatal_provider_failure_is_sanitized_and_durably_completed() {
    let (context, cipher, _) = test_context(true).await;
    let factory = FixtureFactory::new(FixtureMode::FailOverview);
    let clock = fixed_clock();
    let outcome = service(&context, &cipher, &factory, &clock)
        .run_manual()
        .await
        .unwrap();
    let SyncExecution::Failed { run, class } = outcome else {
        panic!("expected failed sync");
    };
    assert_eq!(class, SyncFailureClass::Provider);
    assert_eq!(class.http_status(), StatusCode::BAD_GATEWAY);
    assert_eq!(run.status, SyncRunStatus::Error);
    assert_eq!(
        run.error_message.as_deref(),
        Some("TMO could not complete the synchronization request.")
    );
    let connection = context.connection().await.unwrap();
    assert_eq!(
        scalar_i64(&connection, "SELECT COUNT(*) FROM portfolio_snapshot").await,
        0
    );
    let mut rows = connection
        .query(
            "SELECT status, last_error FROM intg_integration_connection WHERE slug = 'tmo'",
            (),
        )
        .await
        .unwrap();
    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<String>(0).unwrap(), "error");
    assert_eq!(
        row.get::<String>(1).unwrap(),
        "TMO could not complete the synchronization request."
    );
}

#[tokio::test]
async fn scheduled_redelivery_never_calls_the_provider_twice_for_one_slot() {
    let (context, cipher, _) = test_context(true).await;
    OperationRepository::new(&context.connection().await.unwrap())
        .set_scheduler_enabled(true, "2026-07-14T11:59:30.000Z")
        .await
        .unwrap();
    let factory = FixtureFactory::new(FixtureMode::Success);
    let clock = fixed_clock();
    let slot = "2026-07-14T12:00:00.000Z";

    let first = service(&context, &cipher, &factory, &clock)
        .run_scheduled(slot)
        .await
        .unwrap();
    assert!(matches!(first, SyncExecution::Completed(_)));
    let redelivery = service(&context, &cipher, &factory, &clock)
        .run_scheduled(slot)
        .await
        .unwrap();
    let SyncExecution::AlreadyScheduled(existing) = redelivery else {
        panic!("expected the durable slot record to deduplicate redelivery");
    };
    assert_eq!(existing.status, SyncRunStatus::Success);
    assert_eq!(factory.login_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_scheduled_slot_waits_for_the_next_slot_or_manual_retry() {
    let (context, cipher, _) = test_context(true).await;
    OperationRepository::new(&context.connection().await.unwrap())
        .set_scheduler_enabled(true, "2026-07-14T11:59:30.000Z")
        .await
        .unwrap();
    let factory = FixtureFactory::new(FixtureMode::FailOverview);
    let clock = fixed_clock();

    let first = service(&context, &cipher, &factory, &clock)
        .run_scheduled("2026-07-14T12:00:00.000Z")
        .await
        .unwrap();
    assert!(matches!(first, SyncExecution::Failed { .. }));
    let redelivery = service(&context, &cipher, &factory, &clock)
        .run_scheduled("2026-07-14T12:00:00.000Z")
        .await
        .unwrap();
    let SyncExecution::AlreadyScheduled(existing) = redelivery else {
        panic!("expected a failed slot to remain durably deduplicated");
    };
    assert_eq!(existing.status, SyncRunStatus::Error);
    assert_eq!(factory.login_calls.load(Ordering::SeqCst), 1);

    let next_slot = service(&context, &cipher, &factory, &clock)
        .run_scheduled("2026-07-14T18:00:00.000Z")
        .await
        .unwrap();
    assert!(matches!(next_slot, SyncExecution::Failed { .. }));
    assert_eq!(factory.login_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn partial_detail_failure_is_visible_without_discarding_summary_sync() {
    let (context, cipher, _) = test_context(true).await;
    let factory = FixtureFactory::new(FixtureMode::PartialDetail);
    let clock = fixed_clock();
    let outcome = service(&context, &cipher, &factory, &clock)
        .run_manual()
        .await
        .unwrap();
    let SyncExecution::Completed(run) = outcome else {
        panic!("expected successful degraded sync");
    };
    assert_eq!(run.loans_upserted, 2);
    assert_eq!(
        run.endpoints_hit.as_deref(),
        Some("overview,portfolio,loanDetail:1/2,history")
    );
    let connection = context.connection().await.unwrap();
    let mut rows = connection
        .query(
            "SELECT status, last_error FROM intg_integration_connection WHERE slug = 'tmo'",
            (),
        )
        .await
        .unwrap();
    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<String>(0).unwrap(), "degraded");
    assert_eq!(
        row.get::<String>(1).unwrap(),
        "1 TMO loan detail request(s) failed; portfolio summaries were synchronized."
    );
}

#[tokio::test]
async fn overlapping_claim_never_calls_provider() {
    let (context, cipher, _) = test_context(true).await;
    let connection = context.connection().await.unwrap();
    OperationRepository::new(&connection)
        .claim_manual(TMO_CONNECTION_SLUG, "2026-07-14T11:59:59.000Z")
        .await
        .unwrap();
    let factory = FixtureFactory::new(FixtureMode::Success);
    let clock = fixed_clock();
    let outcome = service(&context, &cipher, &factory, &clock)
        .run_manual()
        .await
        .unwrap();
    assert!(matches!(outcome, SyncExecution::AlreadyRunning(_)));
    assert_eq!(factory.login_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        scalar_i64(&connection, "SELECT COUNT(*) FROM sync_log").await,
        1
    );
}

#[tokio::test]
async fn a_claim_exactly_at_the_twenty_minute_boundary_is_still_live() {
    let (context, cipher, _) = test_context(true).await;
    let connection = context.connection().await.unwrap();
    OperationRepository::new(&connection)
        .claim_manual(TMO_CONNECTION_SLUG, "2026-07-14T11:40:00.000Z")
        .await
        .unwrap();
    let factory = FixtureFactory::new(FixtureMode::Success);
    let clock = fixed_clock();
    let outcome = service(&context, &cipher, &factory, &clock)
        .run_manual()
        .await
        .unwrap();
    assert!(matches!(outcome, SyncExecution::AlreadyRunning(_)));
    assert_eq!(factory.login_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn stale_claim_is_interrupted_before_a_new_claim_runs() {
    let (context, cipher, _) = test_context(true).await;
    let connection = context.connection().await.unwrap();
    OperationRepository::new(&connection)
        .claim_manual(TMO_CONNECTION_SLUG, "2026-07-14T11:00:00.000Z")
        .await
        .unwrap();
    let factory = FixtureFactory::new(FixtureMode::Success);
    let clock = fixed_clock();
    let outcome = service(&context, &cipher, &factory, &clock)
        .run_manual()
        .await
        .unwrap();
    assert!(matches!(outcome, SyncExecution::Completed(_)));
    assert_eq!(
        scalar_i64(
            &connection,
            "SELECT COUNT(*) FROM sync_log WHERE status = 'error'"
        )
        .await,
        1
    );
    assert_eq!(
        scalar_i64(
            &connection,
            "SELECT COUNT(*) FROM sync_log WHERE status = 'success'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn invalid_capture_is_a_provider_502_and_commits_no_domain_rows() {
    let (context, cipher, _) = test_context(true).await;
    let factory = FixtureFactory::new(FixtureMode::InvalidDate);
    let clock = fixed_clock();
    let outcome = service(&context, &cipher, &factory, &clock)
        .run_manual()
        .await
        .unwrap();
    let SyncExecution::Failed { run, class } = outcome else {
        panic!("expected invalid-capture failure");
    };
    assert_eq!(class, SyncFailureClass::Provider);
    assert_eq!(class.http_status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        run.error_message.as_deref(),
        Some("TMO returned data that could not be imported safely.")
    );
    let connection = context.connection().await.unwrap();
    assert_eq!(
        scalar_i64(&connection, "SELECT COUNT(*) FROM portfolio_snapshot").await,
        0
    );
    assert_eq!(
        scalar_i64(&connection, "SELECT COUNT(*) FROM intg_tmo_import_loan").await,
        0
    );
}

#[tokio::test]
async fn missing_bootstrap_stream_is_configuration_409() {
    let (context, cipher, _) = test_context(false).await;
    let factory = FixtureFactory::new(FixtureMode::Success);
    let clock = fixed_clock();
    let outcome = service(&context, &cipher, &factory, &clock)
        .run_manual()
        .await
        .unwrap();
    let SyncExecution::Failed { class, .. } = outcome else {
        panic!("expected missing-prerequisite failure");
    };
    assert_eq!(class, SyncFailureClass::Configuration);
    assert_eq!(class.http_status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn completion_race_rolls_back_provider_rows_and_never_overwrites_winner() {
    let (context, cipher, _) = test_context(true).await;
    let factory =
        FixtureFactory::new(FixtureMode::CompletionRace).with_race_context(context.clone());
    let clock = fixed_clock();
    let error = service(&context, &cipher, &factory, &clock)
        .run_manual()
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        SyncRuntimeError::Operation(crate::operations::OperationError::Coordination(_))
    ));
    let connection = context.connection().await.unwrap();
    assert_eq!(
        scalar_i64(&connection, "SELECT COUNT(*) FROM portfolio_snapshot").await,
        0
    );
    assert_eq!(
        scalar_i64(&connection, "SELECT COUNT(*) FROM intg_tmo_import_payment").await,
        0
    );
    let mut rows = connection
        .query("SELECT status, error_message FROM sync_log", ())
        .await
        .unwrap();
    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<String>(0).unwrap(), "error");
    assert_eq!(
        row.get::<String>(1).unwrap(),
        "simulated competing completion"
    );
}

#[test]
fn fixed_test_clock_is_canonical() {
    assert_eq!(
        format_utc_millis(fixed_clock().now()),
        "2026-07-14T12:00:00.000Z"
    );
}
