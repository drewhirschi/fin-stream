CREATE TABLE IF NOT EXISTS intg_loan_workspace (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    connection_id        INTEGER NOT NULL
                             REFERENCES intg_integration_connection(id) ON DELETE CASCADE,
    loan_account         TEXT NOT NULL,
    redfin_url           TEXT,
    zillow_url           TEXT,
    decision_status      TEXT CHECK (
        decision_status IS NULL
        OR decision_status IN ('new', 'reviewing', 'committed', 'funded', 'passed')
    ),
    target_contribution  REAL CHECK (
        target_contribution IS NULL OR abs(target_contribution) < 1.0e308
    ),
    actual_contribution  REAL CHECK (
        actual_contribution IS NULL OR abs(actual_contribution) < 1.0e308
    ),
    notes                TEXT,
    created_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(created_at) >= 20 AND datetime(created_at) IS NOT NULL
    ),
    updated_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(updated_at) >= 20 AND datetime(updated_at) IS NOT NULL
    ),
    CHECK (length(trim(loan_account)) > 0),
    UNIQUE (connection_id, loan_account)
) STRICT;

CREATE INDEX IF NOT EXISTS intg_loan_workspace_connection_idx
    ON intg_loan_workspace (connection_id, loan_account);

CREATE TABLE IF NOT EXISTS intg_loan_workspace_photo (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    connection_id  INTEGER NOT NULL
                       REFERENCES intg_integration_connection(id) ON DELETE CASCADE,
    loan_account   TEXT NOT NULL,
    provider       TEXT NOT NULL,
    caption        TEXT,
    source_url     TEXT NOT NULL,
    image_url      TEXT NOT NULL,
    sort_order     INTEGER NOT NULL DEFAULT 0,
    is_featured    INTEGER NOT NULL DEFAULT 0 CHECK (is_featured IN (0, 1)),
    created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(created_at) >= 20 AND datetime(created_at) IS NOT NULL
    ),
    CHECK (length(trim(loan_account)) > 0),
    CHECK (length(trim(provider)) > 0),
    CHECK (length(source_url) > 0),
    CHECK (length(image_url) > 0),
    UNIQUE (connection_id, loan_account, provider, image_url)
) STRICT;

CREATE INDEX IF NOT EXISTS intg_loan_workspace_photo_workspace_idx
    ON intg_loan_workspace_photo (connection_id, loan_account, sort_order, id);

CREATE UNIQUE INDEX IF NOT EXISTS intg_loan_workspace_photo_one_featured_idx
    ON intg_loan_workspace_photo (connection_id, loan_account)
    WHERE is_featured = 1;

CREATE TABLE IF NOT EXISTS intg_received_email (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    resend_email_id      TEXT NOT NULL UNIQUE,
    from_address         TEXT NOT NULL,
    to_addresses         TEXT NOT NULL CHECK (
        json_valid(to_addresses) = 1 AND json_type(to_addresses) = 'array'
    ),
    subject              TEXT,
    received_at          TEXT NOT NULL CHECK (
        length(received_at) >= 20 AND datetime(received_at) IS NOT NULL
    ),
    body_s3_key          TEXT,
    body_content_type    TEXT,
    loan_account         TEXT,
    processing_state     TEXT NOT NULL DEFAULT 'pending' CHECK (
        processing_state IN ('pending', 'stored', 'error')
    ),
    error_message        TEXT,
    raw_webhook_payload  TEXT CHECK (
        raw_webhook_payload IS NULL OR json_valid(raw_webhook_payload) = 1
    ),
    created_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(created_at) >= 20 AND datetime(created_at) IS NOT NULL
    ),
    updated_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(updated_at) >= 20 AND datetime(updated_at) IS NOT NULL
    ),
    CHECK (length(trim(resend_email_id)) > 0),
    CHECK (length(trim(from_address)) > 0),
    CHECK (loan_account IS NULL OR length(trim(loan_account)) > 0)
) STRICT;

CREATE INDEX IF NOT EXISTS intg_received_email_loan_idx
    ON intg_received_email (loan_account, received_at DESC, id DESC)
    WHERE loan_account IS NOT NULL;

CREATE INDEX IF NOT EXISTS intg_received_email_unlinked_idx
    ON intg_received_email (created_at DESC, id DESC)
    WHERE loan_account IS NULL;

CREATE TABLE IF NOT EXISTS intg_received_email_attachment (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    email_id              INTEGER NOT NULL
                              REFERENCES intg_received_email(id) ON DELETE CASCADE,
    resend_attachment_id  TEXT NOT NULL,
    filename              TEXT NOT NULL,
    content_type          TEXT NOT NULL,
    size_bytes            INTEGER CHECK (size_bytes IS NULL OR size_bytes >= 0),
    s3_key                TEXT,
    processing_state      TEXT NOT NULL DEFAULT 'pending' CHECK (
        processing_state IN ('pending', 'stored', 'error')
    ),
    created_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (
        length(created_at) >= 20 AND datetime(created_at) IS NOT NULL
    ),
    CHECK (length(trim(resend_attachment_id)) > 0),
    CHECK (length(filename) > 0),
    CHECK (length(trim(content_type)) > 0),
    UNIQUE (email_id, resend_attachment_id)
) STRICT;

CREATE INDEX IF NOT EXISTS intg_received_email_attachment_email_idx
    ON intg_received_email_attachment (email_id, id);
