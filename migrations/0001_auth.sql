CREATE TABLE IF NOT EXISTS app_user (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    email TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    display_name TEXT,
    is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS app_session (
    id TEXT PRIMARY KEY,
    data BLOB NOT NULL,
    expires_at_unix_s INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS app_session_expiry_idx ON app_session (expires_at_unix_s);
