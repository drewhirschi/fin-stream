use sqlx::PgPool;

/// Helper: set up a PostgreSQL test database with schema.
///
/// To run these tests, set `TEST_DATABASE_URL`.
async fn setup_db() -> Option<PgPool> {
    let database_url = match std::env::var("TEST_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("Skipping Postgres-backed tests; set TEST_DATABASE_URL to enable them.");
            return None;
        }
    };

    let pool = PgPool::connect(&database_url).await.ok()?;

    // Reset tables for deterministic tests.
    sqlx::query(
        "DROP TABLE IF EXISTS stream_view_stream, stream_view, stream_schedule, stream_event,
         tmo_loan, portfolio_snapshot, settings, account, stream CASCADE",
    )
    .execute(&pool)
    .await
    .ok()?;

    sqlx::query(
        "CREATE TABLE account (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            kind TEXT NOT NULL DEFAULT 'cash',
            balance DOUBLE PRECISION,
            source_type TEXT,
            source_ref TEXT,
            metadata TEXT,
            balance_updated_at TEXT,
            is_primary INTEGER NOT NULL DEFAULT 0,
            is_active INTEGER NOT NULL DEFAULT 1,
            notes TEXT,
            created_at TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(&pool)
    .await
    .ok()?;

    sqlx::query(
        "CREATE TABLE stream (
            id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL, type TEXT NOT NULL,
            kind TEXT, description TEXT, default_account_id BIGINT, configuration TEXT,
            is_active INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(&pool)
    .await
    .ok()?;

    sqlx::query(
        "CREATE TABLE stream_view (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            is_default INTEGER NOT NULL DEFAULT 0,
            is_active INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(&pool)
    .await
    .ok()?;

    sqlx::query(
        "CREATE TABLE stream_view_stream (
            stream_view_id BIGINT NOT NULL REFERENCES stream_view(id) ON DELETE CASCADE,
            stream_id BIGINT NOT NULL REFERENCES stream(id) ON DELETE CASCADE,
            created_at TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            PRIMARY KEY (stream_view_id, stream_id)
        )",
    )
    .execute(&pool)
    .await
    .ok()?;

    sqlx::query(
        "CREATE TABLE stream_event (
            id BIGSERIAL PRIMARY KEY,
            stream_id BIGINT NOT NULL REFERENCES stream(id),
            account_id BIGINT,
            label TEXT, scheduled_date TEXT NOT NULL, expected_date TEXT, actual_date TEXT,
            amount DOUBLE PRECISION NOT NULL, status TEXT NOT NULL DEFAULT 'projected',
            source_id TEXT, source_type TEXT, metadata TEXT, notes TEXT,
            created_at TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            UNIQUE(stream_id, source_type, source_id)
        )",
    )
    .execute(&pool)
    .await
    .ok()?;

    sqlx::query(
        "CREATE TABLE stream_schedule (
            id BIGSERIAL PRIMARY KEY,
            stream_id BIGINT NOT NULL REFERENCES stream(id),
            account_id BIGINT,
            label TEXT,
            amount DOUBLE PRECISION NOT NULL,
            frequency TEXT NOT NULL,
            day_of_month INTEGER,
            start_date TEXT NOT NULL,
            end_date TEXT,
            is_active INTEGER NOT NULL DEFAULT 1,
            metadata TEXT,
            created_at TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(&pool)
    .await
    .ok()?;

    sqlx::query(
        "CREATE TABLE settings (
            key TEXT PRIMARY KEY, value TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(&pool)
    .await
    .ok()?;

    sqlx::query(
        "CREATE TABLE portfolio_snapshot (
            id BIGSERIAL PRIMARY KEY,
            snapshot_date TEXT NOT NULL UNIQUE, portfolio_value DOUBLE PRECISION,
            portfolio_yield DOUBLE PRECISION, portfolio_count INTEGER, ytd_interest DOUBLE PRECISION,
            ytd_principal DOUBLE PRECISION, trust_balance DOUBLE PRECISION, outstanding_checks DOUBLE PRECISION,
            service_fees DOUBLE PRECISION,
            synced_at TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(&pool)
    .await
    .ok()?;

    sqlx::query(
        "CREATE TABLE tmo_loan (
            loan_account TEXT PRIMARY KEY, stream_id BIGINT REFERENCES stream(id),
            borrower_name TEXT, property_address TEXT, property_city TEXT, property_state TEXT,
            property_zip TEXT, property_type TEXT, property_priority INTEGER, occupancy TEXT,
            appraised_value DOUBLE PRECISION, ltv DOUBLE PRECISION, percent_owned DOUBLE PRECISION, loan_type INTEGER,
            note_rate DOUBLE PRECISION, original_balance DOUBLE PRECISION, principal_balance DOUBLE PRECISION,
            regular_payment DOUBLE PRECISION, payment_frequency TEXT DEFAULT 'Monthly',
            maturity_date TEXT, next_payment_date TEXT, interest_paid_to TEXT,
            term_left_months INTEGER, is_delinquent INTEGER DEFAULT 0,
            is_active INTEGER DEFAULT 1, last_synced_at TEXT, detail_synced_at TEXT,
            created_at TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(&pool)
    .await
    .ok()?;

    sqlx::query(
        "INSERT INTO account (id, name, kind, balance, is_primary, is_active)
         VALUES (1, 'Primary Cash', 'cash', NULL, 1, 1)",
    )
    .execute(&pool)
    .await
    .ok()?;

    // Create a test stream
    sqlx::query(
        "INSERT INTO stream (id, name, type, kind, default_account_id)
         VALUES (1, 'Trust Deeds', 'mortgage_portfolio', 'tmo_trust', 1)",
    )
    .execute(&pool)
    .await
    .ok()?;

    // Create an expenses stream
    sqlx::query(
        "INSERT INTO stream (id, name, type, kind, default_account_id)
         VALUES (2, 'One-off Expense', 'manual_expense', 'manual_expense', 1)",
    )
    .execute(&pool)
    .await
    .ok()?;

    Some(pool)
}

/// Insert a test event.
async fn insert_event(
    pool: &PgPool,
    stream_id: i64,
    label: &str,
    scheduled_date: &str,
    amount: f64,
    status: &str,
    source_type: &str,
) -> i64 {
    let source_id = format!("test:{}:{}", label, scheduled_date);
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO stream_event (stream_id, label, scheduled_date, amount, status, source_id, source_type)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(stream_id)
    .bind(label)
    .bind(scheduled_date)
    .bind(amount)
    .bind(status)
    .bind(&source_id)
    .bind(source_type)
    .fetch_one(pool)
    .await
    .unwrap();
    id
}

// ═══════════════════════════════════════════════════════════
// Test 1: Forecast running sum with mixed inflows/outflows
// ═══════════════════════════════════════════════════════════
#[tokio::test]
async fn test_forecast_running_sum() {
    let Some(pool) = setup_db().await else {
        return;
    };

    // Set starting balance
    sqlx::query("INSERT INTO settings (key, value) VALUES ('current_cash', '5000.00')")
        .execute(&pool)
        .await
        .unwrap();

    // 3 inflows + 2 outflows
    insert_event(
        &pool,
        1,
        "Lee payment",
        "2026-04-15",
        396.50,
        "projected",
        "schedule",
    )
    .await;
    insert_event(
        &pool,
        1,
        "Nakagawa payment",
        "2026-04-20",
        1063.00,
        "projected",
        "schedule",
    )
    .await;
    insert_event(
        &pool,
        2,
        "Car insurance",
        "2026-04-18",
        -1200.00,
        "projected",
        "manual",
    )
    .await;
    insert_event(
        &pool,
        1,
        "Carvajal payment",
        "2026-05-01",
        520.00,
        "projected",
        "schedule",
    )
    .await;
    insert_event(
        &pool,
        2,
        "Rent",
        "2026-05-01",
        -2000.00,
        "projected",
        "manual",
    )
    .await;

    let forecast =
        trust_deeds::db::forecasts::compute_forecast(&pool, "2026-04-01", "2026-06-01", None, None)
            .await
            .unwrap()
            .expect("should have forecast");

    assert_eq!(forecast.starting_balance, 5000.0);
    assert_eq!(forecast.rows.len(), 5);

    // Apr 15: 5000 + 396.50 = 5396.50
    assert!((forecast.rows[0].running_balance - 5396.50).abs() < 0.01);
    // Apr 18: 5396.50 - 1200 = 4196.50
    assert!((forecast.rows[1].running_balance - 4196.50).abs() < 0.01);
    // Apr 20: 4196.50 + 1063 = 5259.50
    assert!((forecast.rows[2].running_balance - 5259.50).abs() < 0.01);
    // May 1 (first by id): 5259.50 + 520 = 5779.50
    assert!((forecast.rows[3].running_balance - 5779.50).abs() < 0.01);
    // May 1 (second by id): 5779.50 - 2000 = 3779.50
    assert!((forecast.rows[4].running_balance - 3779.50).abs() < 0.01);

    assert!((forecast.ending_balance - 3779.50).abs() < 0.01);
}

// ═══════════════════════════════════════════════════════════
// Test 2: Empty events returns starting balance unchanged
// ═══════════════════════════════════════════════════════════
#[tokio::test]
async fn test_forecast_empty_events() {
    let Some(pool) = setup_db().await else {
        return;
    };

    sqlx::query("INSERT INTO settings (key, value) VALUES ('current_cash', '10000.00')")
        .execute(&pool)
        .await
        .unwrap();

    let forecast =
        trust_deeds::db::forecasts::compute_forecast(&pool, "2026-04-01", "2026-06-01", None, None)
            .await
            .unwrap()
            .expect("should have forecast");

    assert_eq!(forecast.starting_balance, 10000.0);
    assert_eq!(forecast.rows.len(), 0);
    assert_eq!(forecast.ending_balance, 10000.0);
}

// ═══════════════════════════════════════════════════════════
// Test 3: No starting balance returns None
// ═══════════════════════════════════════════════════════════
#[tokio::test]
async fn test_forecast_no_starting_balance() {
    let Some(pool) = setup_db().await else {
        return;
    };

    let result =
        trust_deeds::db::forecasts::compute_forecast(&pool, "2026-04-01", "2026-06-01", None, None)
            .await
            .unwrap();

    assert!(result.is_none());
}

// ═══════════════════════════════════════════════════════════
// Test 4: Starting balance fallback to portfolio snapshot
// ═══════════════════════════════════════════════════════════
#[tokio::test]
async fn test_forecast_balance_fallback_to_snapshot() {
    let Some(pool) = setup_db().await else {
        return;
    };

    // No settings entry, but a portfolio snapshot exists
    sqlx::query(
        "INSERT INTO portfolio_snapshot (snapshot_date, trust_balance) VALUES ('2026-04-01', 7500.00)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let balance = trust_deeds::db::forecasts::get_starting_balance(&pool).await;
    assert_eq!(balance, Some(7500.0));
}

// ═══════════════════════════════════════════════════════════
// Test 5: Stale projected event cleanup
// ═══════════════════════════════════════════════════════════
#[tokio::test]
async fn test_cleanup_stale_projections() {
    let Some(pool) = setup_db().await else {
        return;
    };

    // Create a past-dated projected event (stale)
    insert_event(
        &pool,
        1,
        "Stale event",
        "2020-01-01",
        100.0,
        "projected",
        "schedule",
    )
    .await;
    // Create a future projected event (should survive)
    insert_event(
        &pool,
        1,
        "Future event",
        "2099-01-01",
        200.0,
        "projected",
        "schedule",
    )
    .await;
    // Create a past received event (should survive - it's not projected)
    insert_event(
        &pool,
        1,
        "Past received",
        "2020-06-01",
        300.0,
        "received",
        "tmo_history",
    )
    .await;

    let deleted = trust_deeds::db::events::cleanup_stale_projections(&pool)
        .await
        .unwrap();

    assert_eq!(deleted, 1);

    // Verify only 2 events remain
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM stream_event")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 2);
}

// ═══════════════════════════════════════════════════════════
// Test 8: Create event
// ═══════════════════════════════════════════════════════════
#[tokio::test]
async fn test_create_event() {
    let Some(pool) = setup_db().await else {
        return;
    };

    let id = trust_deeds::db::events::create_event(
        &pool,
        2,
        None,
        "Car Insurance",
        "2026-05-15",
        -1200.0,
        "projected",
        "manual",
        None,
    )
    .await
    .unwrap();

    assert!(id > 0);

    // Verify it was created
    let (amount,): (f64,) = sqlx::query_as("SELECT amount FROM stream_event WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(amount, -1200.0);
}

// ═══════════════════════════════════════════════════════════
// Test 9: Override event date
// ═══════════════════════════════════════════════════════════
#[tokio::test]
async fn test_override_event_date() {
    let Some(pool) = setup_db().await else {
        return;
    };

    let id = insert_event(
        &pool,
        1,
        "Lee payment",
        "2026-04-15",
        396.50,
        "projected",
        "schedule",
    )
    .await;

    let updated = trust_deeds::db::events::override_event_date(&pool, id, "2026-04-22")
        .await
        .unwrap();

    assert!(updated);

    // Verify expected_date was set
    let (expected,): (Option<String>,) =
        sqlx::query_as("SELECT expected_date FROM stream_event WHERE id = $1")
            .bind(1i64)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(expected.as_deref(), Some("2026-04-22"));
}

// ═══════════════════════════════════════════════════════════
// Test 10: Cannot override received event
// ═══════════════════════════════════════════════════════════
#[tokio::test]
async fn test_cannot_override_received_event() {
    let Some(pool) = setup_db().await else {
        return;
    };

    let id = insert_event(
        &pool,
        1,
        "Lee payment",
        "2026-04-15",
        396.50,
        "received",
        "tmo_history",
    )
    .await;

    let updated = trust_deeds::db::events::override_event_date(&pool, id, "2026-04-22")
        .await
        .unwrap();

    assert!(!updated);
}

// ═══════════════════════════════════════════════════════════
// Test 11: COALESCE ordering - expected_date takes priority
// ═══════════════════════════════════════════════════════════
#[tokio::test]
async fn test_coalesce_ordering() {
    let Some(pool) = setup_db().await else {
        return;
    };

    sqlx::query("INSERT INTO settings (key, value) VALUES ('current_cash', '1000.00')")
        .execute(&pool)
        .await
        .unwrap();

    // Event A: scheduled Apr 15, no override
    insert_event(
        &pool,
        1,
        "Event A",
        "2026-04-15",
        100.0,
        "projected",
        "schedule",
    )
    .await;
    // Event B: scheduled Apr 10, but overridden to Apr 20
    let id_b = insert_event(
        &pool,
        1,
        "Event B",
        "2026-04-10",
        200.0,
        "projected",
        "schedule",
    )
    .await;
    sqlx::query("UPDATE stream_event SET expected_date = '2026-04-20' WHERE id = $1")
        .bind(id_b)
        .execute(&pool)
        .await
        .unwrap();

    let forecast =
        trust_deeds::db::forecasts::compute_forecast(&pool, "2026-04-01", "2026-06-01", None, None)
            .await
            .unwrap()
            .expect("should have forecast");

    // Event A should come first (Apr 15), Event B second (effective Apr 20 due to override)
    assert_eq!(forecast.rows.len(), 2);
    assert_eq!(forecast.rows[0].date, "2026-04-15");
    assert_eq!(forecast.rows[1].date, "2026-04-20");
    assert!((forecast.rows[0].running_balance - 1100.0).abs() < 0.01);
    assert!((forecast.rows[1].running_balance - 1300.0).abs() < 0.01);
}

// ═══════════════════════════════════════════════════════════
// Test 12: Stream filter on forecast
// ═══════════════════════════════════════════════════════════
#[tokio::test]
async fn test_forecast_stream_filter() {
    let Some(pool) = setup_db().await else {
        return;
    };

    sqlx::query("INSERT INTO settings (key, value) VALUES ('current_cash', '5000.00')")
        .execute(&pool)
        .await
        .unwrap();

    insert_event(
        &pool,
        1,
        "Inflow",
        "2026-04-15",
        500.0,
        "projected",
        "schedule",
    )
    .await;
    insert_event(
        &pool,
        2,
        "Expense",
        "2026-04-20",
        -300.0,
        "projected",
        "manual",
    )
    .await;

    // Filter to stream 1 only
    let forecast = trust_deeds::db::forecasts::compute_forecast(
        &pool,
        "2026-04-01",
        "2026-06-01",
        Some(1),
        None,
    )
    .await
    .unwrap()
    .expect("should have forecast");

    assert_eq!(forecast.rows.len(), 1);
    assert_eq!(forecast.rows[0].amount, 500.0);
}

// ═══════════════════════════════════════════════════════════
// Test 13: Set and get starting balance
// ═══════════════════════════════════════════════════════════
#[tokio::test]
async fn test_set_and_get_starting_balance() {
    let Some(pool) = setup_db().await else {
        return;
    };

    // Initially none
    assert!(
        trust_deeds::db::forecasts::get_starting_balance(&pool)
            .await
            .is_none()
    );

    // Set it
    trust_deeds::db::forecasts::set_starting_balance(&pool, 12345.67)
        .await
        .unwrap();

    // Get it back
    let balance = trust_deeds::db::forecasts::get_starting_balance(&pool).await;
    assert!((balance.unwrap() - 12345.67).abs() < 0.01);

    // Update it
    trust_deeds::db::forecasts::set_starting_balance(&pool, 99999.99)
        .await
        .unwrap();
    let balance = trust_deeds::db::forecasts::get_starting_balance(&pool).await;
    assert!((balance.unwrap() - 99999.99).abs() < 0.01);
}
