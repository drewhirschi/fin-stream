-- Income Streams - Database Schema
-- PostgreSQL

-- ============================================================
-- Core Tables
-- ============================================================

CREATE TABLE IF NOT EXISTS stream (
    id          BIGSERIAL PRIMARY KEY,
    name        TEXT    NOT NULL,
    type        TEXT    NOT NULL,  -- 'mortgage_portfolio', 'salary', 'manual'
    description TEXT,
    is_active   INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
    updated_at  TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
);

CREATE TABLE IF NOT EXISTS stream_event (
    id              BIGSERIAL PRIMARY KEY,
    stream_id       BIGINT NOT NULL REFERENCES stream(id),
    label           TEXT,
    scheduled_date  DATE    NOT NULL,
    expected_date   DATE,           -- manual override: when it'll actually land
    actual_date     DATE,           -- when it actually happened
    amount          DOUBLE PRECISION NOT NULL,
    status          TEXT    NOT NULL DEFAULT 'projected',  -- projected, confirmed, received, late, missed
    source_id       TEXT,           -- external ref (check number, etc.)
    source_type     TEXT,           -- 'tmo_history', 'manual', 'schedule'
    metadata        TEXT,           -- JSON blob for type-specific data
    notes           TEXT,
    created_at      TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
    updated_at      TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),

    UNIQUE(stream_id, source_type, source_id)
);

CREATE TABLE IF NOT EXISTS stream_schedule (
    id           BIGSERIAL PRIMARY KEY,
    stream_id    BIGINT NOT NULL REFERENCES stream(id),
    label        TEXT,
    amount       DOUBLE PRECISION NOT NULL,
    frequency    TEXT    NOT NULL,  -- 'monthly', 'bimonthly', 'weekly'
    day_of_month INTEGER,          -- for monthly; -1 = last day
    start_date   TEXT    NOT NULL,
    end_date     TEXT,              -- NULL = indefinite
    is_active    INTEGER NOT NULL DEFAULT 1,
    metadata     TEXT,              -- JSON blob
    created_at   TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
    updated_at   TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
);

-- ============================================================
-- The Mortgage Office (TMO) Tables
-- ============================================================

CREATE TABLE IF NOT EXISTS tmo_account (
    id              BIGINT PRIMARY KEY CHECK (id = 1),
    company_id      TEXT    NOT NULL,
    account_number  TEXT    NOT NULL,
    source_rec_id   TEXT,
    display_name    TEXT,
    email           TEXT,
    last_login_at   TEXT,
    created_at      TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
    updated_at      TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
);

CREATE TABLE IF NOT EXISTS tmo_loan (
    loan_account      TEXT    PRIMARY KEY,
    stream_id         BIGINT REFERENCES stream(id),
    borrower_name     TEXT,
    property_address  TEXT,
    property_city     TEXT,
    property_state    TEXT,
    property_zip      TEXT,
    property_type     TEXT,
    property_priority INTEGER,
    occupancy         TEXT,
    appraised_value   DOUBLE PRECISION,
    ltv               DOUBLE PRECISION,
    percent_owned     DOUBLE PRECISION,
    loan_type         INTEGER,
    note_rate         DOUBLE PRECISION,
    original_balance  DOUBLE PRECISION,
    principal_balance DOUBLE PRECISION,
    regular_payment   DOUBLE PRECISION,
    payment_frequency TEXT    DEFAULT 'Monthly',
    maturity_date     DATE,
    next_payment_date DATE,
    interest_paid_to  DATE,
    term_left_months  INTEGER,
    is_delinquent     INTEGER DEFAULT 0,
    is_active         INTEGER DEFAULT 1,
    last_synced_at    TEXT,
    detail_synced_at  TEXT,
    created_at        TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
    updated_at        TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
);

CREATE TABLE IF NOT EXISTS portfolio_snapshot (
    id                 BIGSERIAL PRIMARY KEY,
    snapshot_date      DATE    NOT NULL UNIQUE,
    portfolio_value    DOUBLE PRECISION,
    portfolio_yield    DOUBLE PRECISION,
    portfolio_count    INTEGER,
    ytd_interest       DOUBLE PRECISION,
    ytd_principal      DOUBLE PRECISION,
    trust_balance      DOUBLE PRECISION,
    outstanding_checks DOUBLE PRECISION,
    service_fees       DOUBLE PRECISION,
    synced_at          TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
);

-- ============================================================
-- Infrastructure
-- ============================================================

CREATE TABLE IF NOT EXISTS sync_log (
    id                BIGSERIAL PRIMARY KEY,
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
    updated_at TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
);

-- ============================================================
-- Indexes
-- ============================================================

-- Timeline queries
CREATE INDEX IF NOT EXISTS idx_event_stream_scheduled ON stream_event(stream_id, scheduled_date);
CREATE INDEX IF NOT EXISTS idx_event_expected         ON stream_event(expected_date) WHERE expected_date IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_event_actual           ON stream_event(actual_date) WHERE actual_date IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_event_status           ON stream_event(status);

-- Schedules
CREATE INDEX IF NOT EXISTS idx_schedule_stream_active ON stream_schedule(stream_id, is_active) WHERE is_active = 1;

-- TMO
CREATE INDEX IF NOT EXISTS idx_tmo_loan_stream ON tmo_loan(stream_id);

-- Snapshots
CREATE INDEX IF NOT EXISTS idx_snapshot_date ON portfolio_snapshot(snapshot_date DESC);

-- Sync log
CREATE INDEX IF NOT EXISTS idx_sync_started ON sync_log(started_at DESC);
