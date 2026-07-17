use libsql::{Builder, Connection, params};

#[cfg(feature = "local-db")]
use crate::config::AppConfig;
use crate::db::AppContext;

use super::{
    AccountDraft, AmountCertainty, Direction, EventDraft, EventPatch, EventStatus, FinanceError,
    FinanceRepository, ForecastQuery, IsoDate, Patch, ProjectionWindow, ScheduleDraft,
    ScheduleFrequency, StreamDraft, StreamViewDraft, verify_foreign_keys,
};

fn date(value: &str) -> IsoDate {
    value.parse().unwrap()
}

async fn test_connection() -> (AppContext, Connection) {
    let database = Builder::new_local(":memory:").build().await.unwrap();
    let context = AppContext::from_database(database).await.unwrap();
    let connection = context.connection().await.unwrap();
    (context, connection)
}

fn stream(name: &str, direction: Direction, schedules: Vec<ScheduleDraft>) -> StreamDraft {
    let kind = match direction {
        Direction::In => "manual_income",
        Direction::Out => "manual_expense",
    };
    StreamDraft {
        id: None,
        name: name.to_owned(),
        stream_type: kind.to_owned(),
        kind: kind.to_owned(),
        direction,
        amount_certainty: AmountCertainty::Known,
        description: None,
        default_account_id: None,
        configuration: None,
        parent_id: None,
        schedules,
    }
}

fn schedule(
    label: &str,
    amount: f64,
    frequency: ScheduleFrequency,
    day_of_month: Option<u8>,
    start_date: &str,
    end_date: Option<&str>,
) -> ScheduleDraft {
    ScheduleDraft {
        id: None,
        account_id: None,
        label: Some(label.to_owned()),
        amount,
        frequency,
        day_of_month,
        start_date: date(start_date),
        end_date: end_date.map(date),
        metadata: None,
    }
}

fn manual_event(stream_id: i64, label: &str, on: &str, amount: f64) -> EventDraft {
    EventDraft {
        stream_id,
        account_id: None,
        label: label.to_owned(),
        expected_date: date(on),
        amount,
        status: EventStatus::Projected,
        metadata: None,
        notes: None,
    }
}

async fn save_stream(
    repository: &FinanceRepository<'_>,
    draft: &StreamDraft,
    from: &str,
    through: &str,
) -> i64 {
    repository
        .save_stream(
            draft,
            ProjectionWindow::new(date(from), date(through)).unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn production_migrations_enforce_constraints_and_foreign_keys() {
    let (_context, connection) = test_connection().await;
    verify_foreign_keys(&connection).await.unwrap();

    let mut version = connection
        .query("SELECT name FROM _schema_migrations WHERE version = 2", ())
        .await
        .unwrap();
    assert_eq!(
        version
            .next()
            .await
            .unwrap()
            .unwrap()
            .get::<String>(0)
            .unwrap(),
        "streams_forecast"
    );

    assert!(
        connection
            .execute(
                "INSERT INTO stream (name, type, kind, direction, amount_certainty) \
                 VALUES ('Bad', 'manual', 'manual', 'sideways', 'known')",
                (),
            )
            .await
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO stream_event ( \
                    stream_id, label, expected_date, amount, source_id, source_type \
                 ) VALUES (999, 'Missing parent', '2026-07-15', 10.0, 'manual:bad', 'manual')",
                (),
            )
            .await
            .is_err()
    );

    let repository = FinanceRepository::new(&connection);
    repository.ensure_primary_account().await.unwrap();
    assert!(
        connection
            .execute(
                "INSERT INTO account (name, kind, is_primary) VALUES ('Other', 'cash', 1)",
                (),
            )
            .await
            .is_err()
    );

    let stream_id = save_stream(
        &repository,
        &stream("Income", Direction::In, Vec::new()),
        "2026-07-01",
        "2026-08-01",
    )
    .await;
    assert!(
        connection
            .execute(
                "INSERT INTO stream_event ( \
                    stream_id, label, expected_date, amount, source_id, source_type \
                 ) VALUES (?1, 'Negative', '2026-07-15', -1.0, 'manual:negative', 'manual')",
                params![stream_id],
            )
            .await
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO stream_schedule ( \
                    stream_id, amount, frequency, day_of_month, start_date \
                 ) VALUES (?1, 10.0, 'fortnightly', NULL, '2026-07-01')",
                params![stream_id],
            )
            .await
            .is_err()
    );
}

#[tokio::test]
#[cfg(feature = "local-db")]
async fn every_app_context_connection_has_foreign_keys_enabled() {
    let unique = format!(
        "trust-deeds-finance-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let path = std::env::temp_dir().join(unique);
    let context = AppContext::connect(&AppConfig {
        cookie_secure: false,
        admin_email: None,
        admin_password: None,
        app_encryption_key: crate::config::AppEncryptionKey::for_test(),
        cron_authenticator: crate::cron_auth::CronAuthenticator::new(None),
        local_database_path: path.clone(),
    })
    .await
    .unwrap();
    let first = context.connection().await.unwrap();
    let second = context.connection().await.unwrap();
    verify_foreign_keys(&first).await.unwrap();
    verify_foreign_keys(&second).await.unwrap();
    drop(first);
    drop(second);
    drop(context);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn bootstrap_is_idempotent_and_does_not_invent_a_zero_cash_anchor() {
    let (_context, connection) = test_connection().await;
    let repository = FinanceRepository::new(&connection);
    let first = repository
        .bootstrap_defaults(date("2026-07-14"))
        .await
        .unwrap();
    let second = repository
        .bootstrap_defaults(date("2026-07-14"))
        .await
        .unwrap();
    assert_eq!(first.primary_account_id, second.primary_account_id);
    assert_eq!(first.default_view_id, second.default_view_id);
    assert_eq!(first.stream_ids, second.stream_ids);

    let accounts = repository.list_accounts().await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].balance, None);
    assert_eq!(accounts[0].balance_as_of_date, None);
    assert!(repository.get_cash_source().await.unwrap().is_none());

    repository
        .set_starting_balance(0.0, date("2026-07-14"), "manual", None, None)
        .await
        .unwrap();
    let source = repository.get_cash_source().await.unwrap().unwrap();
    assert_eq!(source.amount, 0.0);
    assert_eq!(source.as_of_date, "2026-07-14");

    let streams = repository.list_streams().await.unwrap();
    assert_eq!(streams.len(), 6);
    let editors = repository.list_view_editors().await.unwrap();
    assert_eq!(editors.len(), 1);
    assert!(editors[0].is_default == 1);
    assert!(editors[0].members.iter().all(|member| member.included));
}

#[tokio::test]
async fn canvas_stream_catalog_is_small_ordered_and_active_only() {
    let (_context, connection) = test_connection().await;
    connection
        .execute(
            "INSERT INTO stream (name, type, kind, direction, amount_certainty, is_active) VALUES \
                ('Zulu manual', 'manual', 'manual', 'out', 'known', 1), \
                ('Inactive trust', 'mortgage_portfolio', 'tmo_trust', 'in', 'known', 0), \
                ('Card', 'credit_card_due', 'credit_card', 'out', 'estimated', 1), \
                ('Trust Deeds', 'mortgage_portfolio', 'tmo_trust', 'in', 'known', 1), \
                ('Income', 'manual_income', 'manual_income', 'in', 'known', 1)",
            (),
        )
        .await
        .unwrap();

    let streams = FinanceRepository::new(&connection)
        .list_canvas_streams()
        .await
        .unwrap();
    assert_eq!(
        streams
            .iter()
            .map(|stream| (stream.name.as_str(), stream.kind.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("Trust Deeds", "tmo_trust"),
            ("Card", "credit_card"),
            ("Income", "manual_income"),
            ("Zulu manual", "manual"),
        ]
    );
}

#[tokio::test]
async fn primary_switch_and_balance_validation_are_atomic() {
    let (_context, connection) = test_connection().await;
    let repository = FinanceRepository::new(&connection);
    let original = repository.ensure_primary_account().await.unwrap();

    let replacement = repository
        .save_account(&AccountDraft {
            id: None,
            name: "Operating Cash".to_owned(),
            kind: "cash".to_owned(),
            balance: Some(250.0),
            balance_as_of_date: Some(date("2026-07-14")),
            is_primary: true,
            notes: None,
        })
        .await
        .unwrap();
    assert_ne!(original, replacement);
    let accounts = repository.list_accounts().await.unwrap();
    assert_eq!(
        accounts
            .iter()
            .filter(|account| account.is_primary == 1)
            .count(),
        1
    );
    assert_eq!(
        accounts
            .iter()
            .find(|account| account.is_primary == 1)
            .unwrap()
            .id,
        replacement
    );

    let error = repository
        .save_account(&AccountDraft {
            id: Some(replacement),
            name: "Operating Cash".to_owned(),
            kind: "cash".to_owned(),
            balance: Some(250.0),
            balance_as_of_date: Some(date("2026-07-14")),
            is_primary: false,
            notes: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(error, FinanceError::Conflict(_)));
    assert_eq!(
        repository
            .list_accounts()
            .await
            .unwrap()
            .into_iter()
            .filter(|account| account.is_primary == 1)
            .count(),
        1
    );

    let secondary = repository
        .save_account(&AccountDraft {
            id: None,
            name: "Reserve".to_owned(),
            kind: "savings".to_owned(),
            balance: None,
            balance_as_of_date: None,
            is_primary: false,
            notes: None,
        })
        .await
        .unwrap();
    repository
        .save_account(&AccountDraft {
            id: Some(secondary),
            name: "Reserve".to_owned(),
            kind: "savings".to_owned(),
            balance: None,
            balance_as_of_date: None,
            is_primary: true,
            notes: None,
        })
        .await
        .unwrap();
    let accounts = repository.list_accounts().await.unwrap();
    assert_eq!(
        accounts
            .iter()
            .filter(|account| account.is_primary == 1)
            .map(|account| account.id)
            .collect::<Vec<_>>(),
        vec![secondary]
    );

    let error = repository
        .save_account(&AccountDraft {
            id: Some(secondary),
            name: "Broken".to_owned(),
            kind: "savings".to_owned(),
            balance: Some(1.0),
            balance_as_of_date: None,
            is_primary: true,
            notes: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(error, FinanceError::Validation(_)));
    assert_eq!(
        repository
            .list_accounts()
            .await
            .unwrap()
            .into_iter()
            .find(|account| account.id == secondary)
            .unwrap()
            .name,
        "Reserve"
    );
}

#[tokio::test]
async fn unchanged_balance_edit_preserves_provider_provenance() {
    let (_context, connection) = test_connection().await;
    let repository = FinanceRepository::new(&connection);
    let account_id = repository
        .save_account(&AccountDraft {
            id: None,
            name: "Synced Cash".to_owned(),
            kind: "checking".to_owned(),
            balance: Some(250.0),
            balance_as_of_date: Some(date("2026-07-14")),
            is_primary: true,
            notes: None,
        })
        .await
        .unwrap();
    connection
        .execute(
            "UPDATE account SET source_type = 'monarch', source_ref = 'acct-1', \
                    metadata = '{\"pending\":12}', balance_updated_at = '2026-07-14T12:00:00.000Z' \
             WHERE id = ?1",
            params![account_id],
        )
        .await
        .unwrap();

    repository
        .save_account(&AccountDraft {
            id: Some(account_id),
            name: "Renamed Cash".to_owned(),
            kind: "checking".to_owned(),
            balance: Some(250.0),
            balance_as_of_date: Some(date("2026-07-14")),
            is_primary: true,
            notes: Some("Only the label changed".to_owned()),
        })
        .await
        .unwrap();
    let account = repository.list_accounts().await.unwrap().remove(0);
    assert_eq!(account.source_type.as_deref(), Some("monarch"));
    assert_eq!(account.source_ref.as_deref(), Some("acct-1"));
    assert_eq!(account.metadata.as_deref(), Some(r#"{"pending":12}"#));
    assert_eq!(
        account.balance_updated_at.as_deref(),
        Some("2026-07-14T12:00:00.000Z")
    );

    repository
        .save_account(&AccountDraft {
            id: Some(account_id),
            name: "Renamed Cash".to_owned(),
            kind: "checking".to_owned(),
            balance: Some(300.0),
            balance_as_of_date: Some(date("2026-07-15")),
            is_primary: true,
            notes: Some("Only the label changed".to_owned()),
        })
        .await
        .unwrap();
    let account = repository.list_accounts().await.unwrap().remove(0);
    assert_eq!(account.source_type.as_deref(), Some("manual"));
    assert_eq!(account.source_ref, None);
    assert_eq!(account.metadata, None);
}

#[tokio::test]
async fn forecast_signs_magnitudes_and_anchors_past_and_future() {
    let (_context, connection) = test_connection().await;
    let repository = FinanceRepository::new(&connection);
    repository.ensure_primary_account().await.unwrap();
    repository
        .set_starting_balance(1_000.0, date("2026-07-14"), "manual", None, None)
        .await
        .unwrap();
    let income = save_stream(
        &repository,
        &stream("Salary", Direction::In, Vec::new()),
        "2026-07-01",
        "2026-08-01",
    )
    .await;
    let expense = save_stream(
        &repository,
        &stream("Rent", Direction::Out, Vec::new()),
        "2026-07-01",
        "2026-08-01",
    )
    .await;
    repository
        .create_manual_event(&manual_event(expense, "Yesterday", "2026-07-13", 50.0))
        .await
        .unwrap();
    repository
        .create_manual_event(&manual_event(income, "Payday", "2026-07-15", 100.0))
        .await
        .unwrap();
    repository
        .create_manual_event(&manual_event(expense, "Rent", "2026-07-16", 100.0))
        .await
        .unwrap();

    let forecast = repository
        .compute_forecast(ForecastQuery {
            from: date("2026-07-13"),
            through: date("2026-07-16"),
            today: date("2026-07-14"),
            stream_id: None,
            view_id: None,
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(forecast.starting_balance, 1_000.0);
    assert_eq!(forecast.opening_balance, 1_050.0);
    assert_eq!(forecast.balance_as_of_date, "2026-07-14");
    assert_eq!(forecast.ending_balance, 1_000.0);
    assert_eq!(
        forecast
            .rows
            .iter()
            .map(|row| (row.date.as_str(), row.amount, row.running_balance))
            .collect::<Vec<_>>(),
        vec![
            ("2026-07-13", -50.0, 1_000.0),
            ("2026-07-15", 100.0, 1_100.0),
            ("2026-07-16", -100.0, 1_000.0),
        ]
    );

    let zero = repository
        .create_manual_event(&manual_event(income, "No-op", "2026-07-17", 0.0))
        .await
        .unwrap_err();
    assert!(matches!(zero, FinanceError::Validation(_)));
}

#[tokio::test]
async fn forecast_loads_events_between_anchor_and_a_narrow_window() {
    let (_context, connection) = test_connection().await;
    let repository = FinanceRepository::new(&connection);
    repository.ensure_primary_account().await.unwrap();
    repository
        .set_starting_balance(1_000.0, date("2026-07-14"), "manual", None, None)
        .await
        .unwrap();
    let income = save_stream(
        &repository,
        &stream("Income", Direction::In, Vec::new()),
        "2026-07-01",
        "2026-08-01",
    )
    .await;
    let expense = save_stream(
        &repository,
        &stream("Expense", Direction::Out, Vec::new()),
        "2026-07-01",
        "2026-08-01",
    )
    .await;
    repository
        .create_manual_event(&manual_event(income, "Hidden prior", "2026-07-15", 100.0))
        .await
        .unwrap();
    repository
        .create_manual_event(&manual_event(expense, "Visible", "2026-07-16", 50.0))
        .await
        .unwrap();
    let future = repository
        .compute_forecast(ForecastQuery {
            from: date("2026-07-16"),
            through: date("2026-07-16"),
            today: date("2026-07-16"),
            stream_id: None,
            view_id: None,
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(future.rows.len(), 1);
    assert_eq!(future.opening_balance, 1_100.0);
    assert_eq!(future.rows[0].running_balance, 1_050.0);
    assert_eq!(future.ending_balance, 1_050.0);

    repository
        .create_manual_event(&manual_event(income, "Later past", "2026-07-13", 100.0))
        .await
        .unwrap();
    repository
        .create_manual_event(&manual_event(expense, "Old visible", "2026-07-12", 50.0))
        .await
        .unwrap();
    let past = repository
        .compute_forecast(ForecastQuery {
            from: date("2026-07-12"),
            through: date("2026-07-12"),
            today: date("2026-07-16"),
            stream_id: None,
            view_id: None,
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(past.rows.len(), 1);
    assert_eq!(past.opening_balance, 950.0);
    assert_eq!(past.rows[0].running_balance, 900.0);
    assert_eq!(past.ending_balance, 900.0);
}

#[tokio::test]
async fn clearing_an_event_account_override_reinherits_the_stream_default() {
    let (_context, connection) = test_connection().await;
    let repository = FinanceRepository::new(&connection);
    let primary_id = repository.ensure_primary_account().await.unwrap();
    repository
        .set_starting_balance(100.0, date("2026-07-14"), "manual", None, None)
        .await
        .unwrap();
    let override_id = repository
        .save_account(&AccountDraft {
            id: None,
            name: "Override account".to_owned(),
            kind: "checking".to_owned(),
            balance: None,
            balance_as_of_date: None,
            is_primary: false,
            notes: None,
        })
        .await
        .unwrap();
    let mut draft = stream("Income", Direction::In, Vec::new());
    draft.default_account_id = Some(primary_id);
    let stream_id = save_stream(&repository, &draft, "2026-07-01", "2026-08-01").await;
    let event_id = repository
        .create_manual_event(&manual_event(stream_id, "Deposit", "2026-07-15", 10.0))
        .await
        .unwrap();

    repository
        .patch_event(
            event_id,
            &EventPatch {
                account_id: Patch::Set(override_id),
                ..EventPatch::default()
            },
        )
        .await
        .unwrap();
    let overridden = repository
        .compute_forecast(ForecastQuery {
            from: date("2026-07-15"),
            through: date("2026-07-15"),
            today: date("2026-07-14"),
            stream_id: Some(stream_id),
            view_id: None,
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(overridden.rows[0].account_id, Some(override_id));
    assert!(overridden.rows[0].has_account_override);

    repository
        .patch_event(
            event_id,
            &EventPatch {
                account_id: Patch::Clear,
                ..EventPatch::default()
            },
        )
        .await
        .unwrap();
    let inherited = repository
        .compute_forecast(ForecastQuery {
            from: date("2026-07-15"),
            through: date("2026-07-15"),
            today: date("2026-07-14"),
            stream_id: Some(stream_id),
            view_id: None,
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(inherited.rows[0].account_id, Some(primary_id));
    assert!(!inherited.rows[0].has_account_override);

    let mut rows = connection
        .query(
            "SELECT override_account_id, has_account_override FROM stream_event WHERE id = ?1",
            params![event_id],
        )
        .await
        .unwrap();
    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<Option<i64>>(0).unwrap(), None);
    assert_eq!(row.get::<i64>(1).unwrap(), 0);
}

#[tokio::test]
async fn lateness_uses_today_instead_of_the_cash_anchor() {
    let (_context, connection) = test_connection().await;
    let repository = FinanceRepository::new(&connection);
    repository.ensure_primary_account().await.unwrap();
    repository
        .set_starting_balance(100.0, date("2026-07-01"), "manual", None, None)
        .await
        .unwrap();
    let stream_id = save_stream(
        &repository,
        &stream("Income", Direction::In, Vec::new()),
        "2026-07-01",
        "2026-08-01",
    )
    .await;
    repository
        .create_manual_event(&manual_event(
            stream_id,
            "Still expected",
            "2026-07-10",
            10.0,
        ))
        .await
        .unwrap();

    let forecast = repository
        .compute_forecast(ForecastQuery {
            from: date("2026-07-10"),
            through: date("2026-07-10"),
            today: date("2026-07-14"),
            stream_id: Some(stream_id),
            view_id: None,
        })
        .await
        .unwrap()
        .unwrap();
    assert!(forecast.rows[0].is_late);
}

#[tokio::test]
async fn every_supported_schedule_frequency_projects_deterministic_dates() {
    let (_context, connection) = test_connection().await;
    let repository = FinanceRepository::new(&connection);
    let stream_id = save_stream(
        &repository,
        &stream(
            "Cadences",
            Direction::In,
            vec![
                schedule(
                    "monthly",
                    1.0,
                    ScheduleFrequency::Monthly,
                    Some(31),
                    "2024-01-01",
                    Some("2024-04-30"),
                ),
                schedule(
                    "semimonthly",
                    2.0,
                    ScheduleFrequency::Semimonthly,
                    None,
                    "2024-01-10",
                    Some("2024-02-29"),
                ),
                schedule(
                    "biweekly",
                    3.0,
                    ScheduleFrequency::Biweekly,
                    None,
                    "2024-01-03",
                    Some("2024-02-01"),
                ),
                schedule(
                    "weekly",
                    4.0,
                    ScheduleFrequency::Weekly,
                    None,
                    "2024-01-04",
                    Some("2024-01-25"),
                ),
                schedule(
                    "annual",
                    5.0,
                    ScheduleFrequency::Annual,
                    None,
                    "2024-02-29",
                    Some("2025-02-28"),
                ),
                schedule(
                    "one-time",
                    6.0,
                    ScheduleFrequency::OneTime,
                    None,
                    "2024-03-07",
                    None,
                ),
            ],
        ),
        "2024-01-01",
        "2025-02-28",
    )
    .await;
    let events = repository
        .list_events(
            date("2024-01-01"),
            date("2025-02-28"),
            Some(stream_id),
            false,
        )
        .await
        .unwrap();
    let dates_for = |label: &str| {
        events
            .iter()
            .filter(|event| event.label.as_deref() == Some(label))
            .map(|event| event.effective_date.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        dates_for("monthly"),
        vec!["2024-01-31", "2024-02-29", "2024-03-31", "2024-04-30"]
    );
    assert_eq!(
        dates_for("semimonthly"),
        vec!["2024-01-15", "2024-01-31", "2024-02-15", "2024-02-29"]
    );
    assert_eq!(
        dates_for("biweekly"),
        vec!["2024-01-03", "2024-01-17", "2024-01-31"]
    );
    assert_eq!(
        dates_for("weekly"),
        vec!["2024-01-04", "2024-01-11", "2024-01-18", "2024-01-25"]
    );
    assert_eq!(dates_for("annual"), vec!["2024-02-29", "2025-02-28"]);
    assert_eq!(dates_for("one-time"), vec!["2024-03-07"]);

    repository
        .refresh_stream_schedule_events(
            stream_id,
            ProjectionWindow::new(date("2024-01-01"), date("2025-02-28")).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        repository
            .list_events(
                date("2024-01-01"),
                date("2025-02-28"),
                Some(stream_id),
                true,
            )
            .await
            .unwrap()
            .len(),
        events.len(),
        "idempotent refresh must not duplicate deterministic source slots"
    );
}

#[tokio::test]
async fn user_overrides_exclusions_and_reconciliation_survive_schedule_refresh() {
    let (_context, connection) = test_connection().await;
    let repository = FinanceRepository::new(&connection);
    let account_id = repository.ensure_primary_account().await.unwrap();
    repository
        .set_starting_balance(500.0, date("2024-01-01"), "manual", None, None)
        .await
        .unwrap();
    let mut draft = stream(
        "Monthly income",
        Direction::In,
        vec![schedule(
            "scheduled",
            50.0,
            ScheduleFrequency::Monthly,
            Some(15),
            "2024-01-01",
            Some("2024-03-31"),
        )],
    );
    draft.default_account_id = Some(account_id);
    draft.schedules[0].account_id = Some(account_id);
    let stream_id = save_stream(&repository, &draft, "2024-01-01", "2024-03-31").await;
    let initial = repository
        .list_events(
            date("2024-01-01"),
            date("2024-03-31"),
            Some(stream_id),
            true,
        )
        .await
        .unwrap();
    assert_eq!(initial.len(), 3);
    let january_id = initial[0].id;
    let february_id = initial[1].id;
    repository
        .patch_event(
            january_id,
            &EventPatch {
                label: Patch::Set("Moved payment".to_owned()),
                expected_date: Patch::Set(date("2024-01-20")),
                amount: Patch::Set(75.0),
                account_id: Patch::Clear,
                notes: Patch::Set("owner override".to_owned()),
            },
        )
        .await
        .unwrap();
    repository.remove_event(february_id).await.unwrap();
    repository
        .refresh_stream_schedule_events(
            stream_id,
            ProjectionWindow::new(date("2024-01-01"), date("2024-03-31")).unwrap(),
        )
        .await
        .unwrap();
    let refreshed = repository
        .list_events(
            date("2024-01-01"),
            date("2024-03-31"),
            Some(stream_id),
            true,
        )
        .await
        .unwrap();
    let january = refreshed
        .iter()
        .find(|event| event.id == january_id)
        .unwrap();
    assert_eq!(january.expected_date, "2024-01-15");
    assert_eq!(january.effective_date, "2024-01-20");
    assert_eq!(january.label.as_deref(), Some("Moved payment"));
    assert_eq!(january.effective_amount, 75.0);
    assert_eq!(january.account_id, Some(account_id));
    assert!(!january.has_account_override);
    assert!(
        refreshed
            .iter()
            .find(|event| event.id == february_id)
            .unwrap()
            .is_excluded
    );

    let listed = repository.list_streams().await.unwrap();
    let schedule_id = listed[0].schedules[0].id;
    draft.id = Some(stream_id);
    draft.schedules[0].id = Some(schedule_id);
    draft.schedules[0].day_of_month = Some(10);
    repository
        .save_stream(
            &draft,
            ProjectionWindow::new(date("2024-01-01"), date("2024-03-31")).unwrap(),
        )
        .await
        .unwrap();
    let changed = repository
        .list_events(
            date("2024-01-01"),
            date("2024-03-31"),
            Some(stream_id),
            true,
        )
        .await
        .unwrap();
    assert_eq!(changed.len(), 3);
    assert!(changed.iter().any(|event| {
        event.id == january_id
            && event.expected_date == "2024-01-10"
            && event.effective_date == "2024-01-20"
            && !event.is_excluded
    }));
    assert!(
        changed
            .iter()
            .any(|event| { event.expected_date == "2024-02-10" && event.is_excluded })
    );
    assert!(
        changed
            .iter()
            .any(|event| { event.expected_date == "2024-03-10" && !event.is_excluded })
    );

    repository
        .reconcile_event(january_id, date("2024-01-22"), 80.0)
        .await
        .unwrap();
    repository
        .refresh_stream_schedule_events(
            stream_id,
            ProjectionWindow::new(date("2024-01-01"), date("2024-03-31")).unwrap(),
        )
        .await
        .unwrap();
    let reconciled = repository
        .list_events(
            date("2024-01-01"),
            date("2024-03-31"),
            Some(stream_id),
            true,
        )
        .await
        .unwrap()
        .into_iter()
        .find(|event| event.id == january_id)
        .unwrap();
    assert_eq!(reconciled.expected_date, "2024-01-10");
    assert_eq!(reconciled.override_date.as_deref(), Some("2024-01-20"));
    assert_eq!(reconciled.actual_date.as_deref(), Some("2024-01-22"));
    assert_eq!(reconciled.effective_date, "2024-01-22");
    assert_eq!(reconciled.amount, 50.0);
    assert_eq!(reconciled.override_amount, Some(75.0));
    assert_eq!(reconciled.actual_amount, Some(80.0));
    assert_eq!(reconciled.effective_amount, 80.0);

    assert!(matches!(
        repository
            .patch_event(january_id, &EventPatch::default())
            .await
            .unwrap_err(),
        FinanceError::Conflict(_)
    ));
    assert!(matches!(
        repository.remove_event(january_id).await.unwrap_err(),
        FinanceError::Conflict(_)
    ));
    assert!(matches!(
        repository
            .reconcile_event(january_id, date("2024-01-23"), 81.0)
            .await
            .unwrap_err(),
        FinanceError::Conflict(_)
    ));
}

#[tokio::test]
async fn provider_rows_are_immutable_through_canonical_user_mutations() {
    let (_context, connection) = test_connection().await;
    let repository = FinanceRepository::new(&connection);
    let stream_id = save_stream(
        &repository,
        &stream("Provider lane", Direction::In, Vec::new()),
        "2026-07-01",
        "2026-08-01",
    )
    .await;
    let mut rows = connection
        .query(
            "INSERT INTO stream_event ( \
                stream_id, label, expected_date, amount, status, source_id, source_type \
             ) VALUES (?1, 'Provider', '2026-07-20', 10.0, 'projected', 'provider:1', 'provider') \
             RETURNING id",
            params![stream_id],
        )
        .await
        .unwrap();
    let event_id = rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap();
    drop(rows);

    assert!(matches!(
        repository
            .patch_event(
                event_id,
                &EventPatch {
                    amount: Patch::Set(11.0),
                    ..EventPatch::default()
                }
            )
            .await
            .unwrap_err(),
        FinanceError::Conflict(_)
    ));
    assert!(matches!(
        repository.remove_event(event_id).await.unwrap_err(),
        FinanceError::Conflict(_)
    ));
    assert!(matches!(
        repository
            .reconcile_event(event_id, date("2026-07-20"), 10.0)
            .await
            .unwrap_err(),
        FinanceError::Conflict(_)
    ));
    assert!(matches!(
        repository
            .patch_event(999_999, &EventPatch::default())
            .await
            .unwrap_err(),
        FinanceError::NotFound(_)
    ));
}

#[tokio::test]
async fn view_membership_filters_forecast_and_failed_update_rolls_back() {
    let (_context, connection) = test_connection().await;
    let repository = FinanceRepository::new(&connection);
    repository.ensure_primary_account().await.unwrap();
    repository
        .set_starting_balance(100.0, date("2026-07-14"), "manual", None, None)
        .await
        .unwrap();
    let first = save_stream(
        &repository,
        &stream("First", Direction::In, Vec::new()),
        "2026-07-01",
        "2026-08-01",
    )
    .await;
    let second = save_stream(
        &repository,
        &stream("Second", Direction::In, Vec::new()),
        "2026-07-01",
        "2026-08-01",
    )
    .await;
    repository
        .create_manual_event(&manual_event(first, "Included", "2026-07-15", 10.0))
        .await
        .unwrap();
    repository
        .create_manual_event(&manual_event(second, "Excluded", "2026-07-15", 20.0))
        .await
        .unwrap();
    let view_id = repository
        .save_view(&StreamViewDraft {
            id: None,
            name: "Only first".to_owned(),
            description: None,
            is_default: false,
            stream_ids: vec![first],
        })
        .await
        .unwrap();
    let forecast = repository
        .compute_forecast(ForecastQuery {
            from: date("2026-07-14"),
            through: date("2026-07-16"),
            today: date("2026-07-14"),
            stream_id: None,
            view_id: Some(view_id),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(forecast.rows.len(), 1);
    assert_eq!(forecast.rows[0].stream_id, first);
    assert_eq!(forecast.ending_balance, 110.0);

    let error = repository
        .save_view(&StreamViewDraft {
            id: Some(view_id),
            name: "Should roll back".to_owned(),
            description: None,
            is_default: false,
            stream_ids: vec![999_999],
        })
        .await
        .unwrap_err();
    assert!(matches!(error, FinanceError::NotFound(_)));
    let view = repository
        .list_view_summaries()
        .await
        .unwrap()
        .into_iter()
        .find(|view| view.id == view_id)
        .unwrap();
    assert_eq!(view.name, "Only first");
}

#[tokio::test]
async fn cross_stream_schedule_update_is_rejected_and_stream_write_rolls_back() {
    let (_context, connection) = test_connection().await;
    let repository = FinanceRepository::new(&connection);
    let first_draft = stream(
        "First",
        Direction::In,
        vec![schedule(
            "first",
            10.0,
            ScheduleFrequency::Monthly,
            Some(1),
            "2026-07-01",
            None,
        )],
    );
    let second_draft = stream(
        "Second",
        Direction::In,
        vec![schedule(
            "second",
            20.0,
            ScheduleFrequency::Monthly,
            Some(2),
            "2026-07-01",
            None,
        )],
    );
    let first = save_stream(&repository, &first_draft, "2026-07-01", "2026-08-31").await;
    let second = save_stream(&repository, &second_draft, "2026-07-01", "2026-08-31").await;
    let streams = repository.list_streams().await.unwrap();
    let second_schedule_id = streams
        .iter()
        .find(|stream| stream.id == second)
        .unwrap()
        .schedules[0]
        .id;

    let mut invalid = first_draft;
    invalid.id = Some(first);
    invalid.name = "Mutated before failure".to_owned();
    invalid.schedules[0].id = Some(second_schedule_id);
    let error = repository
        .save_stream(
            &invalid,
            ProjectionWindow::new(date("2026-07-01"), date("2026-08-31")).unwrap(),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, FinanceError::NotFound(_)));
    assert_eq!(
        repository
            .list_streams()
            .await
            .unwrap()
            .into_iter()
            .find(|stream| stream.id == first)
            .unwrap()
            .name,
        "First"
    );
}
