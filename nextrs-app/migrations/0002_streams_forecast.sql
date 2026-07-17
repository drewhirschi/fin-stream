PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS account (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    name                TEXT NOT NULL CHECK (length(trim(name)) > 0),
    kind                TEXT NOT NULL DEFAULT 'cash' CHECK (length(trim(kind)) > 0),
    balance             REAL CHECK (balance IS NULL OR abs(balance) < 1.0e308),
    balance_as_of_date  TEXT,
    source_type         TEXT,
    source_ref          TEXT,
    metadata            TEXT CHECK (metadata IS NULL OR json_valid(metadata)),
    balance_updated_at  TEXT,
    is_primary          INTEGER NOT NULL DEFAULT 0 CHECK (is_primary IN (0, 1)),
    is_active           INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    notes               TEXT,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (
        balance_as_of_date IS NULL OR (
            length(balance_as_of_date) = 10
            AND balance_as_of_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
        )
    ),
    CHECK (
        (balance IS NULL AND balance_as_of_date IS NULL)
        OR (balance IS NOT NULL AND balance_as_of_date IS NOT NULL)
    )
) STRICT;

CREATE UNIQUE INDEX IF NOT EXISTS account_one_active_primary_idx
    ON account (is_primary)
    WHERE is_primary = 1 AND is_active = 1;

CREATE INDEX IF NOT EXISTS account_active_name_idx
    ON account (is_active, name COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS stream (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    name                TEXT NOT NULL CHECK (length(trim(name)) > 0),
    type                TEXT NOT NULL CHECK (length(trim(type)) > 0),
    kind                TEXT NOT NULL CHECK (length(trim(kind)) > 0),
    direction           TEXT NOT NULL CHECK (direction IN ('in', 'out')),
    amount_certainty    TEXT NOT NULL CHECK (amount_certainty IN ('known', 'estimated')),
    description         TEXT,
    default_account_id  INTEGER REFERENCES account(id) ON DELETE SET NULL,
    configuration       TEXT CHECK (configuration IS NULL OR json_valid(configuration)),
    parent_id           INTEGER REFERENCES stream(id) ON DELETE SET NULL,
    is_active           INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE INDEX IF NOT EXISTS stream_active_name_idx
    ON stream (is_active, name COLLATE NOCASE);

CREATE INDEX IF NOT EXISTS stream_default_account_idx
    ON stream (default_account_id);

CREATE TABLE IF NOT EXISTS stream_view (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL CHECK (length(trim(name)) > 0),
    description     TEXT,
    is_default      INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    is_active       INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE UNIQUE INDEX IF NOT EXISTS stream_view_one_active_default_idx
    ON stream_view (is_default)
    WHERE is_default = 1 AND is_active = 1;

CREATE TABLE IF NOT EXISTS stream_view_stream (
    stream_view_id  INTEGER NOT NULL REFERENCES stream_view(id) ON DELETE CASCADE,
    stream_id       INTEGER NOT NULL REFERENCES stream(id) ON DELETE CASCADE,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (stream_view_id, stream_id)
) STRICT;

CREATE INDEX IF NOT EXISTS stream_view_stream_stream_idx
    ON stream_view_stream (stream_id, stream_view_id);

CREATE TABLE IF NOT EXISTS stream_schedule (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    stream_id       INTEGER NOT NULL REFERENCES stream(id) ON DELETE CASCADE,
    account_id      INTEGER REFERENCES account(id) ON DELETE SET NULL,
    label           TEXT,
    amount          REAL NOT NULL CHECK (amount >= 0 AND amount < 1.0e308),
    frequency       TEXT NOT NULL CHECK (
        frequency IN ('monthly', 'semimonthly', 'biweekly', 'weekly', 'annual', 'one_time')
    ),
    day_of_month    INTEGER CHECK (day_of_month IS NULL OR day_of_month BETWEEN 1 AND 31),
    start_date      TEXT NOT NULL CHECK (
        length(start_date) = 10
        AND start_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
    ),
    end_date        TEXT CHECK (
        end_date IS NULL OR (
            length(end_date) = 10
            AND end_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
        )
    ),
    is_active       INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    metadata        TEXT CHECK (metadata IS NULL OR json_valid(metadata)),
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (frequency <> 'monthly' OR day_of_month IS NOT NULL),
    CHECK (end_date IS NULL OR end_date >= start_date)
) STRICT;

CREATE INDEX IF NOT EXISTS stream_schedule_active_stream_idx
    ON stream_schedule (stream_id, is_active, id);

CREATE INDEX IF NOT EXISTS stream_schedule_account_idx
    ON stream_schedule (account_id);

CREATE TABLE IF NOT EXISTS stream_event (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    stream_id           INTEGER NOT NULL REFERENCES stream(id) ON DELETE RESTRICT,
    account_id          INTEGER REFERENCES account(id) ON DELETE SET NULL,
    label               TEXT,
    expected_date       TEXT NOT NULL CHECK (
        length(expected_date) = 10
        AND expected_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
    ),
    amount              REAL NOT NULL CHECK (amount >= 0 AND amount < 1.0e308),
    override_label      TEXT,
    has_label_override  INTEGER NOT NULL DEFAULT 0 CHECK (has_label_override IN (0, 1)),
    override_date       TEXT CHECK (
        override_date IS NULL OR (
            length(override_date) = 10
            AND override_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
        )
    ),
    override_amount     REAL CHECK (
        override_amount IS NULL OR (override_amount >= 0 AND override_amount < 1.0e308)
    ),
    override_account_id INTEGER REFERENCES account(id) ON DELETE SET NULL,
    has_account_override INTEGER NOT NULL DEFAULT 0 CHECK (has_account_override IN (0, 1)),
    actual_date         TEXT CHECK (
        actual_date IS NULL OR (
            length(actual_date) = 10
            AND actual_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
        )
    ),
    actual_amount       REAL CHECK (
        actual_amount IS NULL OR (actual_amount >= 0 AND actual_amount < 1.0e308)
    ),
    status              TEXT NOT NULL DEFAULT 'projected' CHECK (
        status IN ('projected', 'confirmed', 'received')
    ),
    is_excluded         INTEGER NOT NULL DEFAULT 0 CHECK (is_excluded IN (0, 1)),
    exclusion_reason    TEXT CHECK (exclusion_reason IN ('user', 'schedule')),
    source_id           TEXT,
    source_type         TEXT,
    metadata            TEXT CHECK (metadata IS NULL OR json_valid(metadata)),
    notes               TEXT,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (stream_id, source_type, source_id),
    CHECK (
        (is_excluded = 0 AND exclusion_reason IS NULL)
        OR (is_excluded = 1 AND exclusion_reason IS NOT NULL)
    ),
    CHECK (has_label_override = 1 OR override_label IS NULL),
    CHECK (has_account_override = 1 OR override_account_id IS NULL),
    CHECK (
        status <> 'received'
        OR (actual_date IS NOT NULL AND actual_amount IS NOT NULL AND is_excluded = 0)
    )
) STRICT;

CREATE INDEX IF NOT EXISTS stream_event_forecast_idx
    ON stream_event (is_excluded, expected_date, stream_id, id);

CREATE INDEX IF NOT EXISTS stream_event_override_date_idx
    ON stream_event (override_date)
    WHERE override_date IS NOT NULL;

CREATE INDEX IF NOT EXISTS stream_event_actual_date_idx
    ON stream_event (actual_date)
    WHERE actual_date IS NOT NULL;

CREATE INDEX IF NOT EXISTS stream_event_account_idx
    ON stream_event (account_id);

CREATE INDEX IF NOT EXISTS stream_event_override_account_idx
    ON stream_event (override_account_id)
    WHERE override_account_id IS NOT NULL;
