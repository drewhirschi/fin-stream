CREATE TABLE IF NOT EXISTS intg_integration_connection (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    slug                TEXT NOT NULL UNIQUE,
    name                TEXT NOT NULL,
    provider            TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'active',
    sync_cadence        TEXT NOT NULL DEFAULT 'manual',
    last_synced_at      TEXT,
    last_error          TEXT,
    metadata            TEXT CHECK (metadata IS NULL OR json_valid(metadata)),
    next_scheduled_at   TEXT CHECK (
        next_scheduled_at IS NULL
        OR (length(next_scheduled_at) >= 20 AND datetime(next_scheduled_at) IS NOT NULL)
    ),
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(created_at) >= 20 AND datetime(created_at) IS NOT NULL
    ),
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(updated_at) >= 20 AND datetime(updated_at) IS NOT NULL
    ),
    CHECK (length(trim(slug)) > 0),
    CHECK (length(trim(name)) > 0),
    CHECK (length(trim(provider)) > 0),
    CHECK (
        last_synced_at IS NULL
        OR (length(last_synced_at) >= 20 AND datetime(last_synced_at) IS NOT NULL)
    )
) STRICT;

CREATE INDEX IF NOT EXISTS intg_integration_connection_status_idx
    ON intg_integration_connection (status, slug);

CREATE TABLE IF NOT EXISTS intg_tmo_import_overview (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    connection_id       INTEGER NOT NULL
                            REFERENCES intg_integration_connection(id) ON DELETE CASCADE,
    snapshot_date       TEXT NOT NULL CHECK (
        length(snapshot_date) = 10
        AND snapshot_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
    ),
    portfolio_value     REAL,
    portfolio_yield     REAL,
    portfolio_count     INTEGER,
    ytd_interest        REAL,
    ytd_principal       REAL,
    trust_balance       REAL,
    outstanding_checks  REAL,
    service_fees        REAL,
    processing_state    TEXT NOT NULL DEFAULT 'captured',
    raw_payload         TEXT CHECK (raw_payload IS NULL OR json_valid(raw_payload)),
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(created_at) >= 20 AND datetime(created_at) IS NOT NULL
    ),
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(updated_at) >= 20 AND datetime(updated_at) IS NOT NULL
    ),
    UNIQUE (connection_id, snapshot_date),
    CHECK (portfolio_value IS NULL OR abs(portfolio_value) < 1.0e308),
    CHECK (portfolio_yield IS NULL OR abs(portfolio_yield) < 1.0e308),
    CHECK (ytd_interest IS NULL OR abs(ytd_interest) < 1.0e308),
    CHECK (ytd_principal IS NULL OR abs(ytd_principal) < 1.0e308),
    CHECK (trust_balance IS NULL OR abs(trust_balance) < 1.0e308),
    CHECK (outstanding_checks IS NULL OR abs(outstanding_checks) < 1.0e308),
    CHECK (service_fees IS NULL OR abs(service_fees) < 1.0e308)
) STRICT;

CREATE INDEX IF NOT EXISTS intg_tmo_import_overview_connection_idx
    ON intg_tmo_import_overview (connection_id, snapshot_date DESC);

CREATE TABLE IF NOT EXISTS intg_tmo_import_loan (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    connection_id           INTEGER NOT NULL
                                REFERENCES intg_integration_connection(id) ON DELETE CASCADE,
    stream_id               INTEGER REFERENCES stream(id),
    loan_account            TEXT NOT NULL,
    borrower_name           TEXT,
    property_address        TEXT,
    property_city           TEXT,
    property_state          TEXT,
    property_zip            TEXT,
    property_description    TEXT,
    property_type           TEXT,
    property_priority       INTEGER,
    occupancy               TEXT,
    appraised_value         REAL,
    ltv                     REAL,
    percent_owned           REAL,
    priority                INTEGER,
    loan_type               INTEGER,
    interest_rate           REAL,
    note_rate               REAL,
    original_balance        REAL,
    loan_balance            REAL,
    principal_balance       REAL,
    regular_payment         REAL,
    payment_frequency       TEXT DEFAULT 'Monthly',
    maturity_date           TEXT CHECK (
        maturity_date IS NULL OR (
            length(maturity_date) = 10
            AND maturity_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
        )
    ),
    next_payment_date       TEXT CHECK (
        next_payment_date IS NULL OR (
            length(next_payment_date) = 10
            AND next_payment_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
        )
    ),
    interest_paid_to        TEXT CHECK (
        interest_paid_to IS NULL OR (
            length(interest_paid_to) = 10
            AND interest_paid_to GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
        )
    ),
    billed_through          TEXT CHECK (
        billed_through IS NULL OR (
            length(billed_through) = 10
            AND billed_through GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
        )
    ),
    term_left_months        INTEGER,
    is_delinquent           INTEGER DEFAULT 0 CHECK (is_delinquent IN (0, 1)),
    is_active               INTEGER DEFAULT 1 CHECK (is_active IN (0, 1)),
    raw_summary_payload     TEXT CHECK (
        raw_summary_payload IS NULL OR json_valid(raw_summary_payload)
    ),
    raw_detail_payload      TEXT CHECK (
        raw_detail_payload IS NULL OR json_valid(raw_detail_payload)
    ),
    summary_imported_at     TEXT CHECK (
        summary_imported_at IS NULL
        OR (length(summary_imported_at) >= 20 AND datetime(summary_imported_at) IS NOT NULL)
    ),
    detail_imported_at      TEXT CHECK (
        detail_imported_at IS NULL
        OR (length(detail_imported_at) >= 20 AND datetime(detail_imported_at) IS NOT NULL)
    ),
    created_at              TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(created_at) >= 20 AND datetime(created_at) IS NOT NULL
    ),
    updated_at              TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(updated_at) >= 20 AND datetime(updated_at) IS NOT NULL
    ),
    UNIQUE (connection_id, loan_account),
    CHECK (appraised_value IS NULL OR abs(appraised_value) < 1.0e308),
    CHECK (ltv IS NULL OR abs(ltv) < 1.0e308),
    CHECK (percent_owned IS NULL OR abs(percent_owned) < 1.0e308),
    CHECK (interest_rate IS NULL OR abs(interest_rate) < 1.0e308),
    CHECK (note_rate IS NULL OR abs(note_rate) < 1.0e308),
    CHECK (original_balance IS NULL OR abs(original_balance) < 1.0e308),
    CHECK (loan_balance IS NULL OR abs(loan_balance) < 1.0e308),
    CHECK (principal_balance IS NULL OR abs(principal_balance) < 1.0e308),
    CHECK (regular_payment IS NULL OR abs(regular_payment) < 1.0e308)
) STRICT;

CREATE INDEX IF NOT EXISTS intg_tmo_import_loan_connection_idx
    ON intg_tmo_import_loan (connection_id, loan_account);

CREATE INDEX IF NOT EXISTS intg_tmo_import_loan_stream_idx
    ON intg_tmo_import_loan (stream_id);

CREATE TABLE IF NOT EXISTS intg_tmo_import_payment (
    id                          INTEGER PRIMARY KEY AUTOINCREMENT,
    connection_id               INTEGER NOT NULL
                                     REFERENCES intg_integration_connection(id) ON DELETE CASCADE,
    external_id                 TEXT NOT NULL,
    loan_account                TEXT NOT NULL,
    borrower_name               TEXT NOT NULL,
    property_name               TEXT NOT NULL,
    check_number                TEXT,
    check_date                  TEXT NOT NULL CHECK (
        length(check_date) = 10
        AND check_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
    ),
    amount                      REAL NOT NULL,
    service_fee                 REAL NOT NULL,
    interest                    REAL NOT NULL,
    principal                   REAL NOT NULL,
    charges                     REAL NOT NULL,
    late_charges                REAL NOT NULL,
    other                       REAL NOT NULL,
    processing_state            TEXT NOT NULL DEFAULT 'captured',
    normalized_event_source_id  TEXT,
    raw_payload                 TEXT CHECK (raw_payload IS NULL OR json_valid(raw_payload)),
    imported_at                 TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(imported_at) >= 20 AND datetime(imported_at) IS NOT NULL
    ),
    updated_at                  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(updated_at) >= 20 AND datetime(updated_at) IS NOT NULL
    ),
    UNIQUE (connection_id, external_id),
    CHECK (abs(amount) < 1.0e308),
    CHECK (abs(service_fee) < 1.0e308),
    CHECK (abs(interest) < 1.0e308),
    CHECK (abs(principal) < 1.0e308),
    CHECK (abs(charges) < 1.0e308),
    CHECK (abs(late_charges) < 1.0e308),
    CHECK (abs(other) < 1.0e308)
) STRICT;

CREATE INDEX IF NOT EXISTS intg_tmo_import_payment_connection_idx
    ON intg_tmo_import_payment (connection_id, check_date DESC);

CREATE INDEX IF NOT EXISTS intg_tmo_import_payment_state_idx
    ON intg_tmo_import_payment (processing_state);

CREATE TABLE IF NOT EXISTS intg_tmo_account (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    company_id      TEXT NOT NULL,
    account_number  TEXT NOT NULL,
    source_rec_id   TEXT,
    display_name    TEXT,
    email           TEXT,
    last_login_at   TEXT CHECK (
        last_login_at IS NULL
        OR (length(last_login_at) >= 20 AND datetime(last_login_at) IS NOT NULL)
    ),
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(created_at) >= 20 AND datetime(created_at) IS NOT NULL
    ),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(updated_at) >= 20 AND datetime(updated_at) IS NOT NULL
    )
) STRICT;

CREATE TABLE IF NOT EXISTS intg_tmo_credential (
    connection_id   INTEGER PRIMARY KEY
                        REFERENCES intg_integration_connection(id) ON DELETE CASCADE,
    company_id      TEXT NOT NULL,
    account_number  TEXT NOT NULL,
    pin_ciphertext  TEXT NOT NULL,
    pin_nonce       TEXT NOT NULL,
    key_version     INTEGER NOT NULL DEFAULT 1 CHECK (key_version > 0),
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(created_at) >= 20 AND datetime(created_at) IS NOT NULL
    ),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(updated_at) >= 20 AND datetime(updated_at) IS NOT NULL
    )
) STRICT;

CREATE INDEX IF NOT EXISTS intg_tmo_credential_account_idx
    ON intg_tmo_credential (account_number);

CREATE TABLE IF NOT EXISTS intg_monarch_credential (
    connection_id            INTEGER PRIMARY KEY
                                 REFERENCES intg_integration_connection(id) ON DELETE CASCADE,
    access_token_ciphertext  TEXT NOT NULL,
    access_token_nonce       TEXT NOT NULL,
    default_account_id       TEXT NOT NULL,
    key_version              INTEGER NOT NULL DEFAULT 1 CHECK (key_version > 0),
    created_at               TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(created_at) >= 20 AND datetime(created_at) IS NOT NULL
    ),
    updated_at               TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(updated_at) >= 20 AND datetime(updated_at) IS NOT NULL
    )
) STRICT;

CREATE INDEX IF NOT EXISTS intg_monarch_credential_default_account_idx
    ON intg_monarch_credential (default_account_id);

CREATE TABLE IF NOT EXISTS intg_tmo_payment_event_link (
    tmo_payment_id   INTEGER PRIMARY KEY
                         REFERENCES intg_tmo_import_payment(id) ON DELETE CASCADE,
    stream_event_id  INTEGER NOT NULL REFERENCES stream_event(id) ON DELETE CASCADE,
    created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(created_at) >= 20 AND datetime(created_at) IS NOT NULL
    )
) STRICT;

CREATE INDEX IF NOT EXISTS intg_tmo_payment_event_link_event_idx
    ON intg_tmo_payment_event_link (stream_event_id);

CREATE TABLE IF NOT EXISTS portfolio_snapshot (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    snapshot_date       TEXT NOT NULL UNIQUE CHECK (
        length(snapshot_date) = 10
        AND snapshot_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
    ),
    portfolio_value     REAL,
    portfolio_yield     REAL,
    portfolio_count     INTEGER,
    ytd_interest        REAL,
    ytd_principal       REAL,
    trust_balance       REAL,
    outstanding_checks  REAL,
    service_fees        REAL,
    synced_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(synced_at) >= 20 AND datetime(synced_at) IS NOT NULL
    ),
    CHECK (portfolio_value IS NULL OR abs(portfolio_value) < 1.0e308),
    CHECK (portfolio_yield IS NULL OR abs(portfolio_yield) < 1.0e308),
    CHECK (ytd_interest IS NULL OR abs(ytd_interest) < 1.0e308),
    CHECK (ytd_principal IS NULL OR abs(ytd_principal) < 1.0e308),
    CHECK (trust_balance IS NULL OR abs(trust_balance) < 1.0e308),
    CHECK (outstanding_checks IS NULL OR abs(outstanding_checks) < 1.0e308),
    CHECK (service_fees IS NULL OR abs(service_fees) < 1.0e308)
) STRICT;

CREATE INDEX IF NOT EXISTS portfolio_snapshot_date_idx
    ON portfolio_snapshot (snapshot_date DESC);

CREATE TABLE IF NOT EXISTS settings (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(updated_at) >= 20 AND datetime(updated_at) IS NOT NULL
    ),
    CHECK (length(key) > 0)
) STRICT;

-- This is the durable execution record as well as the one-running guard.
-- Provider work happens outside short database transactions; only claims and
-- conditional state transitions are persisted here.
CREATE TABLE IF NOT EXISTS sync_log (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    connection_slug     TEXT NOT NULL,
    scheduled_for       TEXT CHECK (
        scheduled_for IS NULL OR (
            length(scheduled_for) = 24
            AND scheduled_for GLOB '????-??-??T??:??:??.???Z'
            AND datetime(scheduled_for) IS NOT NULL
        )
    ),
    started_at          TEXT NOT NULL CHECK (
        length(started_at) = 24
        AND started_at GLOB '????-??-??T??:??:??.???Z'
        AND datetime(started_at) IS NOT NULL
    ),
    finished_at         TEXT CHECK (
        finished_at IS NULL OR (
            length(finished_at) = 24
            AND finished_at GLOB '????-??-??T??:??:??.???Z'
            AND datetime(finished_at) IS NOT NULL
        )
    ),
    status              TEXT NOT NULL CHECK (status IN ('running', 'success', 'error')),
    error_message       TEXT,
    endpoints_hit       TEXT,
    events_upserted     INTEGER NOT NULL DEFAULT 0 CHECK (events_upserted >= 0),
    loans_upserted      INTEGER NOT NULL DEFAULT 0 CHECK (loans_upserted >= 0),
    snapshots_created   INTEGER NOT NULL DEFAULT 0 CHECK (snapshots_created >= 0),
    CHECK (length(trim(connection_slug)) > 0),
    CHECK (
        (status = 'running' AND finished_at IS NULL)
        OR (status IN ('success', 'error') AND finished_at IS NOT NULL)
    )
) STRICT;

CREATE UNIQUE INDEX IF NOT EXISTS sync_log_one_running_per_connection_idx
    ON sync_log (connection_slug)
    WHERE status = 'running';

CREATE UNIQUE INDEX IF NOT EXISTS sync_log_one_scheduled_slot_idx
    ON sync_log (connection_slug, scheduled_for)
    WHERE scheduled_for IS NOT NULL;

CREATE INDEX IF NOT EXISTS sync_log_connection_started_idx
    ON sync_log (connection_slug, started_at DESC);

-- A production import remains inert until an operator explicitly enables it.
-- Scheduler enablement is separate so manual verification can precede cron.
CREATE TABLE IF NOT EXISTS operation_control (
    id                  INTEGER PRIMARY KEY CHECK (id = 1),
    mode                TEXT NOT NULL CHECK (mode IN ('read_only', 'enabled')),
    scheduler_enabled   INTEGER NOT NULL CHECK (scheduler_enabled IN (0, 1)),
    updated_at          TEXT NOT NULL CHECK (
        length(updated_at) = 24
        AND updated_at GLOB '????-??-??T??:??:??.???Z'
        AND datetime(updated_at) IS NOT NULL
    ),
    CHECK (mode = 'enabled' OR scheduler_enabled = 0)
) STRICT;

INSERT INTO operation_control (id, mode, scheduler_enabled, updated_at)
VALUES (1, 'read_only', 0, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
ON CONFLICT (id) DO NOTHING;
