-- Income Streams - Database Schema
-- SQLite 3

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA synchronous = NORMAL;

-- ============================================================
-- Core Tables
-- ============================================================

CREATE TABLE IF NOT EXISTS stream (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL,
    type        TEXT    NOT NULL,  -- 'mortgage_portfolio', 'salary', 'manual'
    description TEXT,
    is_active   INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS stream_event (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    stream_id       INTEGER NOT NULL REFERENCES stream(id),
    label           TEXT,
    scheduled_date  TEXT    NOT NULL,
    expected_date   TEXT,           -- manual override: when it'll actually land
    actual_date     TEXT,           -- when it actually happened
    amount          REAL    NOT NULL,
    status          TEXT    NOT NULL DEFAULT 'projected',  -- projected, confirmed, received, late, missed
    source_id       TEXT,           -- external ref (check number, etc.)
    source_type     TEXT,           -- 'tmo_history', 'manual', 'schedule'
    metadata        TEXT,           -- JSON blob for type-specific data
    notes           TEXT,
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),

    UNIQUE(stream_id, source_type, source_id)
);

CREATE TABLE IF NOT EXISTS stream_schedule (
    id           INTEGER PRIMARY KEY,
    stream_id    INTEGER NOT NULL REFERENCES stream(id),
    label        TEXT,
    amount       REAL    NOT NULL,
    frequency    TEXT    NOT NULL,  -- 'monthly', 'bimonthly', 'weekly'
    day_of_month INTEGER,          -- for monthly; -1 = last day
    start_date   TEXT    NOT NULL,
    end_date     TEXT,              -- NULL = indefinite
    is_active    INTEGER NOT NULL DEFAULT 1,
    metadata     TEXT,              -- JSON blob
    created_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- ============================================================
-- The Mortgage Office (TMO) Tables
-- ============================================================

CREATE TABLE IF NOT EXISTS tmo_account (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    company_id      TEXT    NOT NULL,
    account_number  TEXT    NOT NULL,
    source_rec_id   TEXT,
    display_name    TEXT,
    email           TEXT,
    last_login_at   TEXT,
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS tmo_loan (
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
);

CREATE TABLE IF NOT EXISTS portfolio_snapshot (
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
);

-- ============================================================
-- Infrastructure
-- ============================================================

CREATE TABLE IF NOT EXISTS sync_log (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at        TEXT    NOT NULL,
    finished_at       TEXT,
    status            TEXT    NOT NULL DEFAULT 'running',
    error_message     TEXT,
    endpoints_hit     TEXT,
    events_upserted   INTEGER DEFAULT 0,
    loans_upserted    INTEGER DEFAULT 0,
    snapshots_created INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- ============================================================
-- Indexes
-- ============================================================

-- Timeline queries
CREATE INDEX IF NOT EXISTS idx_event_stream_scheduled ON stream_event(stream_id, scheduled_date);
CREATE INDEX IF NOT EXISTS idx_event_expected         ON stream_event(expected_date) WHERE expected_date IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_event_actual            ON stream_event(actual_date) WHERE actual_date IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_event_status            ON stream_event(status);

-- Schedules
CREATE INDEX IF NOT EXISTS idx_schedule_stream_active ON stream_schedule(stream_id, is_active) WHERE is_active = 1;

-- TMO
CREATE INDEX IF NOT EXISTS idx_tmo_loan_stream ON tmo_loan(stream_id);

-- Snapshots
CREATE INDEX IF NOT EXISTS idx_snapshot_date ON portfolio_snapshot(snapshot_date DESC);

-- Sync log
CREATE INDEX IF NOT EXISTS idx_sync_started ON sync_log(started_at DESC);
