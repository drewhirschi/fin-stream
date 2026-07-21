use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use libsql::{Builder, Connection};

use super::{
    ClaimOutcome, OperationError, OperationMode, OperationRepository, SyncCompletion,
    SyncRunStatus, utc_now_millis,
};
use crate::db::AppContext;

static NEXT_DATABASE_ID: AtomicU64 = AtomicU64::new(1);

async fn test_connection() -> Connection {
    let database = Builder::new_local(":memory:").build().await.unwrap();
    AppContext::from_database(database)
        .await
        .unwrap()
        .connection()
        .await
        .unwrap()
}

async fn seed_connection(connection: &Connection, slug: &str) {
    connection
        .execute(
            "INSERT INTO intg_integration_connection (slug, name, provider) \
             VALUES (?1, ?1, ?1)",
            libsql::params![slug],
        )
        .await
        .unwrap();
}

async fn enable_operations(connection: &Connection, scheduler_enabled: bool) {
    OperationRepository::new(connection)
        .set_control(
            OperationMode::Enabled,
            scheduler_enabled,
            "2026-07-14T18:00:00.000Z",
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn operation_control_defaults_read_only_and_guards_claims() {
    let connection = test_connection().await;
    seed_connection(&connection, "tmo").await;
    let repository = OperationRepository::new(&connection);

    let control = repository.control().await.unwrap();
    assert_eq!(control.mode, OperationMode::ReadOnly);
    assert!(!control.scheduler_enabled);
    assert!(matches!(
        repository
            .claim_manual("tmo", "2026-07-14T18:00:00.000Z")
            .await,
        Err(OperationError::ReadOnly)
    ));
    assert!(matches!(
        repository.claim_manual("tmo", "2026-07-14T18:00:00Z").await,
        Err(OperationError::Validation(_))
    ));
    assert!(matches!(
        repository
            .set_control(OperationMode::ReadOnly, true, "2026-07-14T18:00:00.000Z")
            .await,
        Err(OperationError::Validation(_))
    ));

    repository
        .set_control(OperationMode::Enabled, false, "2026-07-14T18:01:00.000Z")
        .await
        .unwrap();
    assert!(matches!(
        repository
            .claim_scheduled(
                "tmo",
                "2026-07-14T18:00:00.000Z",
                "2026-07-14T18:01:00.000Z"
            )
            .await,
        Err(OperationError::SchedulerDisabled)
    ));
    assert!(matches!(
        repository
            .claim_manual("missing", "2026-07-14T18:00:00.000Z")
            .await,
        Err(OperationError::ConnectionNotFound(slug)) if slug == "missing"
    ));
}

#[tokio::test]
async fn operator_transitions_keep_scheduler_separate_from_write_mode() {
    let connection = test_connection().await;
    let repository = OperationRepository::new(&connection);

    assert!(matches!(
        repository
            .set_scheduler_enabled(true, "2026-07-14T18:00:00.000Z")
            .await,
        Err(OperationError::ReadOnly)
    ));

    let enabled = repository
        .enable_writes("2026-07-14T18:01:00.000Z")
        .await
        .unwrap();
    assert_eq!(enabled.mode, OperationMode::Enabled);
    assert!(!enabled.scheduler_enabled);

    let scheduled = repository
        .set_scheduler_enabled(true, "2026-07-14T18:02:00.000Z")
        .await
        .unwrap();
    assert!(scheduled.scheduler_enabled);

    let frozen = repository
        .enter_read_only("2026-07-14T18:03:00.000Z")
        .await
        .unwrap();
    assert_eq!(frozen.mode, OperationMode::ReadOnly);
    assert!(!frozen.scheduler_enabled);
}

#[tokio::test]
async fn operation_control_changes_are_durable_across_file_connections() {
    let id = NEXT_DATABASE_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "trust-deeds-operation-control-{}-{id}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);

    let initial_database = Builder::new_local(&path).build().await.unwrap();
    let context = AppContext::from_database(initial_database).await.unwrap();
    let first = context.connection().await.unwrap();
    let second_database = Builder::new_local(&path).build().await.unwrap();
    let second = second_database.connect().unwrap();
    second
        .execute("PRAGMA foreign_keys = ON", ())
        .await
        .unwrap();

    OperationRepository::new(&first)
        .enable_writes("2026-07-14T18:00:00.000Z")
        .await
        .unwrap();
    assert_eq!(
        OperationRepository::new(&second)
            .control()
            .await
            .unwrap()
            .mode,
        OperationMode::Enabled
    );

    OperationRepository::new(&second)
        .enter_read_only("2026-07-14T18:01:00.000Z")
        .await
        .unwrap();
    assert_eq!(
        OperationRepository::new(&first)
            .control()
            .await
            .unwrap()
            .mode,
        OperationMode::ReadOnly
    );

    let now = utc_now_millis();
    assert_eq!(now.len(), 24);
    OperationRepository::new(&first)
        .enable_writes(&now)
        .await
        .unwrap();

    drop(second);
    drop(second_database);
    drop(first);
    drop(context);
    remove_database_files(&path);
}

#[tokio::test]
async fn concurrent_manual_claims_use_separate_file_connections() {
    let id = NEXT_DATABASE_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "trust-deeds-operation-claims-{}-{id}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);

    let initial_database = Builder::new_local(&path).build().await.unwrap();
    let context = AppContext::from_database(initial_database).await.unwrap();
    let setup = context.connection().await.unwrap();
    seed_connection(&setup, "tmo").await;
    enable_operations(&setup, false).await;

    let database = Builder::new_local(&path).build().await.unwrap();
    let first = database.connect().unwrap();
    let second = database.connect().unwrap();
    for connection in [&first, &second] {
        connection
            .execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")
            .await
            .unwrap();
    }
    let first_repository = OperationRepository::new(&first);
    let second_repository = OperationRepository::new(&second);
    let (first_outcome, second_outcome) = tokio::join!(
        first_repository.claim_manual("tmo", "2026-07-14T18:00:00.000Z"),
        second_repository.claim_manual("tmo", "2026-07-14T18:00:00.001Z")
    );
    let outcomes = [first_outcome.unwrap(), second_outcome.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ClaimOutcome::Claimed(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ClaimOutcome::AlreadyRunning(_)))
            .count(),
        1
    );

    let mut rows = first
        .query(
            "SELECT COUNT(*) FROM sync_log WHERE connection_slug = 'tmo' AND status = 'running'",
            (),
        )
        .await
        .unwrap();
    assert_eq!(
        rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
        1
    );

    drop(first);
    drop(second);
    drop(database);
    drop(setup);
    drop(context);
    remove_database_files(&path);
}

#[tokio::test]
async fn scheduled_claims_dedupe_and_manual_success_covers_a_slot() {
    let connection = test_connection().await;
    seed_connection(&connection, "tmo").await;
    enable_operations(&connection, true).await;
    let repository = OperationRepository::new(&connection);

    let ClaimOutcome::Claimed(manual) = repository
        .claim_manual("tmo", "2026-07-14T18:05:00.000Z")
        .await
        .unwrap()
    else {
        panic!("manual run was not claimed");
    };
    repository
        .complete_success(
            manual.id,
            "2026-07-14T18:06:00.000Z",
            &SyncCompletion::default(),
        )
        .await
        .unwrap();

    let coverage = repository
        .claim_scheduled(
            "tmo",
            "2026-07-14T18:00:00.000Z",
            "2026-07-14T18:07:00.000Z",
        )
        .await
        .unwrap();
    assert!(matches!(coverage, ClaimOutcome::CoveredBySuccess(run) if run.id == manual.id));

    let slot = "2026-07-15T00:00:00.000Z";
    let mut last_error_id = 0;
    for attempt in 0..super::MAX_SCHEDULED_ATTEMPTS {
        let started_at = format!("2026-07-15T00:0{}:00.000Z", attempt + 1);
        let finished_at = format!("2026-07-15T00:0{}:30.000Z", attempt + 1);
        let ClaimOutcome::Claimed(scheduled) = repository
            .claim_scheduled("tmo", slot, &started_at)
            .await
            .unwrap()
        else {
            panic!("scheduled attempt {attempt} was not claimed");
        };
        repository
            .complete_error(scheduled.id, &finished_at, "provider unavailable")
            .await
            .unwrap();
        last_error_id = scheduled.id;
    }
    let exhausted = repository
        .claim_scheduled("tmo", slot, "2026-07-15T00:09:00.000Z")
        .await
        .unwrap();
    assert!(matches!(
        exhausted,
        ClaimOutcome::AlreadyScheduled(run)
            if run.id == last_error_id && run.status == SyncRunStatus::Error
    ));
}

#[tokio::test]
async fn stale_transition_uses_caller_cutoff_and_keeps_newer_run() {
    let connection = test_connection().await;
    seed_connection(&connection, "tmo").await;
    seed_connection(&connection, "monarch").await;
    enable_operations(&connection, false).await;
    let repository = OperationRepository::new(&connection);

    let ClaimOutcome::Claimed(old) = repository
        .claim_manual("tmo", "2026-07-14T18:00:00.000Z")
        .await
        .unwrap()
    else {
        panic!("old run was not claimed");
    };
    let ClaimOutcome::Claimed(newer) = repository
        .claim_manual("monarch", "2026-07-14T18:00:00.000Z")
        .await
        .unwrap()
    else {
        panic!("newer run was not claimed");
    };

    let interrupted = repository
        .interrupt_stale(
            "tmo",
            "2026-07-14T18:20:00.000Z",
            "2026-07-14T18:05:00.000Z",
        )
        .await
        .unwrap();
    assert_eq!(interrupted.len(), 1);
    assert_eq!(interrupted[0].id, old.id);
    assert_eq!(interrupted[0].status, SyncRunStatus::Error);
    assert_eq!(
        interrupted[0].error_message.as_deref(),
        Some("execution owner expired before recording completion")
    );
    // The other connection's run is equally old, but a slug-scoped interrupt
    // never reaps it.
    assert_eq!(
        repository.current_run("monarch").await.unwrap().unwrap().id,
        newer.id
    );
    assert!(repository.current_run("tmo").await.unwrap().is_none());
}

#[tokio::test]
async fn completion_is_conditional_and_schema_rejects_impossible_states() {
    let connection = test_connection().await;
    seed_connection(&connection, "tmo").await;
    enable_operations(&connection, false).await;
    let repository = OperationRepository::new(&connection);
    let ClaimOutcome::Claimed(run) = repository
        .claim_manual("tmo", "2026-07-14T18:00:00.000Z")
        .await
        .unwrap()
    else {
        panic!("run was not claimed");
    };

    let completed = repository
        .complete_success(
            run.id,
            "2026-07-14T18:01:00.000Z",
            &SyncCompletion {
                endpoints_hit: Some("overview,portfolio".to_owned()),
                events_upserted: 12,
                loans_upserted: 4,
                snapshots_created: 1,
            },
        )
        .await
        .unwrap();
    assert_eq!(completed.status, SyncRunStatus::Success);
    assert_eq!(completed.events_upserted, 12);
    assert!(matches!(
        repository
            .complete_error(run.id, "2026-07-14T18:02:00.000Z", "late second completion")
            .await,
        Err(OperationError::Coordination(_))
    ));
    assert!(
        connection
            .execute(
                "INSERT INTO sync_log (connection_slug, started_at, finished_at, status) \
                 VALUES ('tmo', '2026-07-14T19:00:00.000Z', NULL, 'success')",
                (),
            )
            .await
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO sync_log (connection_slug, started_at, finished_at, status) \
                 VALUES ('tmo', '2026-07-14T19:00:00.000Z', \
                         '2026-07-14T19:01:00.000Z', 'running')",
                (),
            )
            .await
            .is_err()
    );
    assert_eq!(repository.list_recent("tmo", 20).await.unwrap().len(), 1);
    assert_eq!(repository.sync_run(run.id).await.unwrap(), Some(completed));
}

fn remove_database_files(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
}
