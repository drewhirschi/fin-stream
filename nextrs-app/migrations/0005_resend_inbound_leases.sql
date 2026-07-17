CREATE TABLE IF NOT EXISTS intg_received_email_processing_lease (
    email_id      INTEGER PRIMARY KEY
                      REFERENCES intg_received_email(id) ON DELETE CASCADE,
    lease_token   TEXT NOT NULL UNIQUE CHECK (
        length(lease_token) >= 32 AND length(lease_token) <= 128
    ),
    claimed_at    TEXT NOT NULL CHECK (
        length(claimed_at) >= 20 AND datetime(claimed_at) IS NOT NULL
    )
) STRICT;

CREATE INDEX IF NOT EXISTS intg_received_email_processing_lease_claimed_idx
    ON intg_received_email_processing_lease (claimed_at, email_id);
