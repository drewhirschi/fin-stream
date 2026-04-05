pub mod events;
pub mod forecasts;
pub mod loans;

use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

use crate::config;

pub async fn init() -> anyhow::Result<SqlitePool> {
    let url = config::get_database_url();

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await?;

    sqlx::query("PRAGMA journal_mode = WAL").execute(&pool).await?;
    sqlx::query("PRAGMA foreign_keys = ON").execute(&pool).await?;
    sqlx::query("PRAGMA busy_timeout = 5000").execute(&pool).await?;
    sqlx::query("PRAGMA synchronous = NORMAL").execute(&pool).await?;

    run_migrations(&pool).await?;

    tracing::info!("database initialized");
    Ok(pool)
}

async fn run_migrations(pool: &SqlitePool) -> anyhow::Result<()> {
    // Create tables inline so we don't need sqlx-cli for this personal tool
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS stream (
            id          INTEGER PRIMARY KEY,
            name        TEXT    NOT NULL,
            type        TEXT    NOT NULL,
            description TEXT,
            is_active   INTEGER NOT NULL DEFAULT 1,
            created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS stream_event (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            stream_id       INTEGER NOT NULL REFERENCES stream(id),
            label           TEXT,
            scheduled_date  TEXT    NOT NULL,
            expected_date   TEXT,
            actual_date     TEXT,
            amount          REAL    NOT NULL,
            status          TEXT    NOT NULL DEFAULT 'projected',
            source_id       TEXT,
            source_type     TEXT,
            metadata        TEXT,
            notes           TEXT,
            created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            updated_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            UNIQUE(stream_id, source_type, source_id)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS stream_schedule (
            id           INTEGER PRIMARY KEY,
            stream_id    INTEGER NOT NULL REFERENCES stream(id),
            label        TEXT,
            amount       REAL    NOT NULL,
            frequency    TEXT    NOT NULL,
            day_of_month INTEGER,
            start_date   TEXT    NOT NULL,
            end_date     TEXT,
            is_active    INTEGER NOT NULL DEFAULT 1,
            metadata     TEXT,
            created_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            updated_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tmo_account (
            id              INTEGER PRIMARY KEY CHECK (id = 1),
            company_id      TEXT    NOT NULL,
            account_number  TEXT    NOT NULL,
            source_rec_id   TEXT,
            display_name    TEXT,
            email           TEXT,
            last_login_at   TEXT,
            created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            updated_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tmo_loan (
            loan_account      TEXT    PRIMARY KEY,
            stream_id         INTEGER REFERENCES stream(id),
            borrower_name     TEXT,
            property_address  TEXT,
            property_city     TEXT,
            property_state    TEXT,
            property_zip      TEXT,
            property_type     TEXT,
            property_priority INTEGER,
            occupancy         TEXT,
            appraised_value   REAL,
            ltv               REAL,
            percent_owned     REAL,
            loan_type         INTEGER,
            note_rate         REAL,
            original_balance  REAL,
            principal_balance REAL,
            regular_payment   REAL,
            payment_frequency TEXT    DEFAULT 'Monthly',
            maturity_date     TEXT,
            next_payment_date TEXT,
            interest_paid_to  TEXT,
            term_left_months  INTEGER,
            is_delinquent     INTEGER DEFAULT 0,
            is_active         INTEGER DEFAULT 1,
            last_synced_at    TEXT,
            detail_synced_at  TEXT,
            created_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            updated_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS portfolio_snapshot (
            id                 INTEGER PRIMARY KEY AUTOINCREMENT,
            snapshot_date      TEXT    NOT NULL UNIQUE,
            portfolio_value    REAL,
            portfolio_yield    REAL,
            portfolio_count    INTEGER,
            ytd_interest       REAL,
            ytd_principal      REAL,
            trust_balance      REAL,
            outstanding_checks REAL,
            service_fees       REAL,
            synced_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sync_log (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            started_at        TEXT    NOT NULL,
            finished_at       TEXT,
            status            TEXT    NOT NULL DEFAULT 'running',
            error_message     TEXT,
            endpoints_hit     TEXT,
            events_upserted   INTEGER DEFAULT 0,
            loans_upserted    INTEGER DEFAULT 0,
            snapshots_created INTEGER DEFAULT 0
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS settings (
            key        TEXT PRIMARY KEY,
            value      TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        )",
    )
    .execute(pool)
    .await?;

    // Indexes
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_event_stream_scheduled ON stream_event(stream_id, scheduled_date)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_event_expected ON stream_event(expected_date) WHERE expected_date IS NOT NULL").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_event_actual ON stream_event(actual_date) WHERE actual_date IS NOT NULL").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_event_status ON stream_event(status)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_schedule_stream_active ON stream_schedule(stream_id, is_active) WHERE is_active = 1").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tmo_loan_stream ON tmo_loan(stream_id)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_snapshot_date ON portfolio_snapshot(snapshot_date DESC)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_sync_started ON sync_log(started_at DESC)").execute(pool).await?;

    Ok(())
}

/// Ensure the "Trustee Income" stream exists, return its id.
pub async fn ensure_trustee_stream(pool: &SqlitePool) -> anyhow::Result<i64> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM stream WHERE type = 'mortgage_portfolio' LIMIT 1")
            .fetch_optional(pool)
            .await?;

    if let Some((id,)) = row {
        return Ok(id);
    }

    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO stream (name, type, description) VALUES (?, ?, ?) RETURNING id",
    )
    .bind("Trustee Income")
    .bind("mortgage_portfolio")
    .bind("Mortgage loan payments via Val-Chris Investments / The Mortgage Office")
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// Ensure the "Expenses" stream exists, return its id.
pub async fn ensure_expenses_stream(pool: &SqlitePool) -> anyhow::Result<i64> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM stream WHERE type = 'expenses' LIMIT 1")
            .fetch_optional(pool)
            .await?;

    if let Some((id,)) = row {
        return Ok(id);
    }

    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO stream (name, type, description) VALUES (?, ?, ?) RETURNING id",
    )
    .bind("Expenses")
    .bind("expenses")
    .bind("Manual expenses and bills")
    .fetch_one(pool)
    .await?;

    Ok(id)
}
