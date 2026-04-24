pub mod accounts;
pub mod emails;
pub mod events;
pub mod forecasts;
pub mod integrations;
pub mod streams;
pub mod users;
pub mod workspaces;

use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

use crate::config;

pub async fn init() -> anyhow::Result<PgPool> {
    let url = config::get_database_url();

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&url)
        .await
        .map_err(|error| anyhow::anyhow!("database connection failed for {}: {}", url, error))?;

    run_migrations(&pool).await?;

    tracing::info!("database initialized");
    Ok(pool)
}

async fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
    // Create tables inline so we don't need sqlx-cli for this personal tool
    sqlx::query("CREATE SCHEMA IF NOT EXISTS intg")
        .execute(pool)
        .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS account (
            id                 BIGSERIAL PRIMARY KEY,
            name               TEXT    NOT NULL,
            kind               TEXT    NOT NULL DEFAULT 'cash',
            balance            DOUBLE PRECISION,
            source_type        TEXT,
            source_ref         TEXT,
            metadata           TEXT,
            balance_updated_at TEXT,
            is_primary         INTEGER NOT NULL DEFAULT 0,
            is_active          INTEGER NOT NULL DEFAULT 1,
            notes              TEXT,
            created_at         TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at         TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS stream (
            id          BIGSERIAL PRIMARY KEY,
            name        TEXT    NOT NULL,
            type        TEXT    NOT NULL,
            kind        TEXT,
            description TEXT,
            default_account_id BIGINT,
            configuration TEXT,
            is_active   INTEGER NOT NULL DEFAULT 1,
            created_at  TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at  TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS stream_view (
            id          BIGSERIAL PRIMARY KEY,
            name        TEXT    NOT NULL,
            description TEXT,
            is_default  INTEGER NOT NULL DEFAULT 0,
            is_active   INTEGER NOT NULL DEFAULT 1,
            created_at  TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at  TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS stream_view_stream (
            stream_view_id BIGINT NOT NULL REFERENCES stream_view(id) ON DELETE CASCADE,
            stream_id      BIGINT NOT NULL REFERENCES stream(id) ON DELETE CASCADE,
            created_at     TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            PRIMARY KEY (stream_view_id, stream_id)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS stream_event (
            id              BIGSERIAL PRIMARY KEY,
            stream_id       BIGINT NOT NULL REFERENCES stream(id),
            account_id      BIGINT,
            label           TEXT,
            expected_date   DATE    NOT NULL,
            actual_date     DATE,
            amount          DOUBLE PRECISION NOT NULL,
            status          TEXT    NOT NULL DEFAULT 'projected',
            source_id       TEXT,
            source_type     TEXT,
            metadata        TEXT,
            notes           TEXT,
            created_at      TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at      TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            UNIQUE(stream_id, source_type, source_id)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS stream_schedule (
            id           BIGSERIAL PRIMARY KEY,
            stream_id    BIGINT NOT NULL REFERENCES stream(id),
            account_id   BIGINT,
            label        TEXT,
            amount       DOUBLE PRECISION NOT NULL,
            frequency    TEXT    NOT NULL,
            day_of_month INTEGER,
            start_date   DATE    NOT NULL,
            end_date     DATE,
            is_active    INTEGER NOT NULL DEFAULT 1,
            metadata     TEXT,
            created_at   TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at   TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query("ALTER TABLE stream ADD COLUMN IF NOT EXISTS kind TEXT")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE stream ADD COLUMN IF NOT EXISTS default_account_id BIGINT")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE stream ADD COLUMN IF NOT EXISTS configuration TEXT")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE stream ADD COLUMN IF NOT EXISTS parent_id BIGINT REFERENCES stream(id)")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE stream_event ADD COLUMN IF NOT EXISTS account_id BIGINT")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE stream_schedule ADD COLUMN IF NOT EXISTS account_id BIGINT")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE account ADD COLUMN IF NOT EXISTS metadata TEXT")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE IF EXISTS intg.tmo_import_loan ADD COLUMN IF NOT EXISTS loan_balance DOUBLE PRECISION")
        .execute(pool)
        .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS intg.integration_connection (
            id              BIGSERIAL PRIMARY KEY,
            slug            TEXT NOT NULL UNIQUE,
            name            TEXT NOT NULL,
            provider        TEXT NOT NULL,
            status          TEXT NOT NULL DEFAULT 'active',
            sync_cadence    TEXT NOT NULL DEFAULT 'manual',
            last_synced_at  TEXT,
            last_error      TEXT,
            metadata        TEXT,
            created_at      TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at      TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "ALTER TABLE intg.integration_connection
         ADD COLUMN IF NOT EXISTS sync_cadence TEXT NOT NULL DEFAULT 'manual'",
    )
    .execute(pool)
    .await?;

    // Observability-only: last time the scheduler projected when the next run
    // would fire. Cron cadence remains the source of truth; this is just so
    // the UI can say "next sync expected at <time>".
    sqlx::query(
        "ALTER TABLE intg.integration_connection
         ADD COLUMN IF NOT EXISTS next_scheduled_at TEXT",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS intg.tmo_import_overview (
            id                 BIGSERIAL PRIMARY KEY,
            connection_id      BIGINT NOT NULL REFERENCES intg.integration_connection(id) ON DELETE CASCADE,
            snapshot_date      DATE NOT NULL,
            portfolio_value    DOUBLE PRECISION,
            portfolio_yield    DOUBLE PRECISION,
            portfolio_count    INTEGER,
            ytd_interest       DOUBLE PRECISION,
            ytd_principal      DOUBLE PRECISION,
            trust_balance      DOUBLE PRECISION,
            outstanding_checks DOUBLE PRECISION,
            service_fees       DOUBLE PRECISION,
            processing_state   TEXT NOT NULL DEFAULT 'captured',
            raw_payload        TEXT,
            created_at         TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at         TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            UNIQUE(connection_id, snapshot_date)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS intg.tmo_import_loan (
            id                  BIGSERIAL PRIMARY KEY,
            connection_id       BIGINT NOT NULL REFERENCES intg.integration_connection(id) ON DELETE CASCADE,
            stream_id           BIGINT REFERENCES stream(id),
            loan_account        TEXT NOT NULL,
            borrower_name       TEXT,
            property_address    TEXT,
            property_city       TEXT,
            property_state      TEXT,
            property_zip        TEXT,
            property_description TEXT,
            property_type       TEXT,
            property_priority   INTEGER,
            occupancy           TEXT,
            appraised_value     DOUBLE PRECISION,
            ltv                 DOUBLE PRECISION,
            percent_owned       DOUBLE PRECISION,
            priority            INTEGER,
            loan_type           INTEGER,
            interest_rate       DOUBLE PRECISION,
            note_rate           DOUBLE PRECISION,
            original_balance    DOUBLE PRECISION,
            principal_balance   DOUBLE PRECISION,
            regular_payment     DOUBLE PRECISION,
            payment_frequency   TEXT DEFAULT 'Monthly',
            maturity_date       DATE,
            next_payment_date   DATE,
            interest_paid_to    DATE,
            billed_through      DATE,
            term_left_months    INTEGER,
            is_delinquent       INTEGER DEFAULT 0,
            is_active           INTEGER DEFAULT 1,
            raw_summary_payload TEXT,
            raw_detail_payload  TEXT,
            summary_imported_at TEXT,
            detail_imported_at  TEXT,
            created_at          TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at          TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            UNIQUE(connection_id, loan_account)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS intg.tmo_import_payment (
            id                         BIGSERIAL PRIMARY KEY,
            connection_id              BIGINT NOT NULL REFERENCES intg.integration_connection(id) ON DELETE CASCADE,
            external_id                TEXT NOT NULL,
            loan_account               TEXT NOT NULL,
            borrower_name              TEXT NOT NULL,
            property_name              TEXT NOT NULL,
            check_number               TEXT NOT NULL,
            check_date                 DATE NOT NULL,
            amount                     DOUBLE PRECISION NOT NULL,
            service_fee                DOUBLE PRECISION NOT NULL,
            interest                   DOUBLE PRECISION NOT NULL,
            principal                  DOUBLE PRECISION NOT NULL,
            charges                    DOUBLE PRECISION NOT NULL,
            late_charges               DOUBLE PRECISION NOT NULL,
            other                      DOUBLE PRECISION NOT NULL,
            processing_state           TEXT NOT NULL DEFAULT 'captured',
            normalized_event_source_id TEXT,
            raw_payload                TEXT,
            imported_at                TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at                 TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            UNIQUE(connection_id, external_id)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS intg.tmo_account (
            id              BIGINT PRIMARY KEY CHECK (id = 1),
            company_id      TEXT NOT NULL,
            account_number  TEXT NOT NULL,
            source_rec_id   TEXT,
            display_name    TEXT,
            email           TEXT,
            last_login_at   TEXT,
            created_at      TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at      TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS intg.tmo_credential (
            connection_id     BIGINT PRIMARY KEY REFERENCES intg.integration_connection(id) ON DELETE CASCADE,
            company_id        TEXT NOT NULL,
            account_number    TEXT NOT NULL,
            pin_ciphertext    TEXT NOT NULL,
            pin_nonce         TEXT NOT NULL,
            key_version       INTEGER NOT NULL DEFAULT 1,
            created_at        TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at        TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS intg.monarch_credential (
            connection_id             BIGINT PRIMARY KEY REFERENCES intg.integration_connection(id) ON DELETE CASCADE,
            access_token_ciphertext   TEXT NOT NULL,
            access_token_nonce        TEXT NOT NULL,
            default_account_id        TEXT NOT NULL,
            key_version               INTEGER NOT NULL DEFAULT 1,
            created_at                TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at                TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS intg.loan_workspace (
            id                  BIGSERIAL PRIMARY KEY,
            connection_id       BIGINT NOT NULL REFERENCES intg.integration_connection(id) ON DELETE CASCADE,
            loan_account        TEXT NOT NULL,
            redfin_url          TEXT,
            zillow_url          TEXT,
            decision_status     TEXT,
            target_contribution DOUBLE PRECISION,
            actual_contribution DOUBLE PRECISION,
            notes               TEXT,
            created_at          TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at          TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            UNIQUE(connection_id, loan_account)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS intg.loan_workspace_photo (
            id             BIGSERIAL PRIMARY KEY,
            connection_id  BIGINT NOT NULL REFERENCES intg.integration_connection(id) ON DELETE CASCADE,
            loan_account   TEXT NOT NULL,
            provider       TEXT NOT NULL,
            caption        TEXT,
            source_url     TEXT NOT NULL,
            image_url      TEXT NOT NULL,
            sort_order     INTEGER NOT NULL DEFAULT 0,
            is_featured    BOOLEAN NOT NULL DEFAULT FALSE,
            created_at     TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            UNIQUE(connection_id, loan_account, provider, image_url)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "ALTER TABLE intg.loan_workspace_photo
         ADD COLUMN IF NOT EXISTS is_featured BOOLEAN NOT NULL DEFAULT FALSE",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "DO $$
         BEGIN
             IF EXISTS (
                 SELECT 1 FROM information_schema.tables
                 WHERE table_schema = 'public' AND table_name = 'integration_connection'
             ) THEN
                 INSERT INTO intg.integration_connection (id, slug, name, provider, status, last_synced_at, last_error, metadata, created_at, updated_at)
                 SELECT id, slug, name, provider, status, last_synced_at, last_error, metadata, created_at, updated_at
                 FROM public.integration_connection
                 ON CONFLICT (slug) DO NOTHING;
             END IF;
         END $$",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "DO $$
         BEGIN
             IF EXISTS (
                 SELECT 1 FROM information_schema.tables
                 WHERE table_schema = 'public' AND table_name IN ('tmo_import_payment', 'tmo_loan', 'tmo_account')
             ) THEN
                 INSERT INTO intg.integration_connection (slug, name, provider)
                 VALUES ('tmo', 'The Mortgage Office', 'mortgage_office')
                 ON CONFLICT (slug) DO NOTHING;
             END IF;
         END $$",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "DO $$
         BEGIN
             IF EXISTS (
                 SELECT 1 FROM information_schema.tables
                 WHERE table_schema = 'public' AND table_name = 'tmo_import_payment'
             ) THEN
                 INSERT INTO intg.tmo_import_payment (
                     connection_id, external_id, loan_account, borrower_name, property_name,
                     check_number, check_date, amount, service_fee, interest, principal,
                     charges, late_charges, other, processing_state, normalized_event_source_id,
                     raw_payload, imported_at, updated_at
                 )
                 SELECT connection_id, external_id, loan_account, borrower_name, property_name,
                        check_number, split_part(check_date::text, 'T', 1)::date, amount, service_fee, interest, principal,
                        charges, late_charges, other, processing_state, normalized_event_source_id,
                        raw_payload, imported_at, updated_at
                 FROM public.tmo_import_payment
                 ON CONFLICT (connection_id, external_id) DO NOTHING;
             END IF;
         END $$",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "DO $$
         BEGIN
             IF EXISTS (
                 SELECT 1 FROM information_schema.tables
                 WHERE table_schema = 'public' AND table_name = 'tmo_loan'
             ) THEN
                 INSERT INTO intg.tmo_import_loan (
                     connection_id, stream_id, loan_account, borrower_name, property_address, property_city,
                     property_state, property_zip, property_type, property_priority, occupancy,
                     appraised_value, ltv, percent_owned, loan_type, note_rate, original_balance,
                     principal_balance, regular_payment, payment_frequency, maturity_date,
                     next_payment_date, interest_paid_to, term_left_months, is_delinquent, is_active,
                     summary_imported_at, detail_imported_at, created_at, updated_at
                 )
                 SELECT
                     COALESCE((SELECT id FROM intg.integration_connection WHERE slug = 'tmo' LIMIT 1), 1),
                     stream_id, loan_account, borrower_name, property_address, property_city,
                     property_state, property_zip, property_type, property_priority, occupancy,
                     appraised_value, ltv, percent_owned, loan_type, note_rate, original_balance,
                     principal_balance, regular_payment, payment_frequency,
                     CASE WHEN maturity_date IS NULL THEN NULL ELSE split_part(maturity_date::text, 'T', 1)::date END,
                     CASE WHEN next_payment_date IS NULL THEN NULL ELSE split_part(next_payment_date::text, 'T', 1)::date END,
                     CASE WHEN interest_paid_to IS NULL THEN NULL ELSE split_part(interest_paid_to::text, 'T', 1)::date END,
                     term_left_months, is_delinquent, is_active,
                     last_synced_at, detail_synced_at, created_at, updated_at
                 FROM public.tmo_loan
                 ON CONFLICT (connection_id, loan_account) DO NOTHING;
             END IF;
         END $$",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "DO $$
         BEGIN
             IF EXISTS (
                 SELECT 1 FROM information_schema.tables
                 WHERE table_schema = 'public' AND table_name = 'tmo_account'
             ) THEN
                 INSERT INTO intg.tmo_account (
                     id, company_id, account_number, source_rec_id, display_name, email,
                     last_login_at, created_at, updated_at
                 )
                 SELECT id, company_id, account_number, source_rec_id, display_name, email,
                        last_login_at, created_at, updated_at
                 FROM public.tmo_account
                 ON CONFLICT (id) DO NOTHING;
             END IF;
         END $$",
    )
    .execute(pool)
    .await?;

    sqlx::query("DROP TABLE IF EXISTS public.integration_record")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS public.tmo_import_payment")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS public.tmo_loan")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS public.tmo_account")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS public.integration_connection")
        .execute(pool)
        .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS portfolio_snapshot (
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
            synced_at          TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sync_log (
            id                BIGSERIAL PRIMARY KEY,
            connection_slug   TEXT,
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

    sqlx::query("ALTER TABLE sync_log ADD COLUMN IF NOT EXISTS connection_slug TEXT")
        .execute(pool)
        .await?;

    // Legacy scheduled_date type migration — only runs if the column still
    // exists (pre-quarantine schemas). After the two-date migration below,
    // scheduled_date is dropped entirely.
    sqlx::query(
        "DO $$
         BEGIN
             IF EXISTS (
                 SELECT 1 FROM information_schema.columns
                 WHERE table_name = 'stream_event' AND column_name = 'scheduled_date'
             ) THEN
                 EXECUTE 'ALTER TABLE stream_event ALTER COLUMN scheduled_date TYPE DATE USING split_part(scheduled_date::text, ''T'', 1)::date';
             END IF;
         END $$",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "ALTER TABLE stream_event
         ALTER COLUMN expected_date TYPE DATE
         USING CASE
             WHEN expected_date IS NULL OR expected_date::text = '' THEN NULL
             ELSE split_part(expected_date::text, 'T', 1)::date
         END",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "ALTER TABLE stream_event
         ALTER COLUMN actual_date TYPE DATE
         USING CASE
             WHEN actual_date IS NULL OR actual_date::text = '' THEN NULL
             ELSE split_part(actual_date::text, 'T', 1)::date
         END",
    )
    .execute(pool)
    .await?;

    // Two-date migration: backfill expected_date from scheduled_date for any
    // legacy rows that only had scheduled_date populated, then make expected_date
    // NOT NULL and drop scheduled_date. Idempotent — safe to re-run.
    sqlx::query(
        "DO $$
         BEGIN
             IF EXISTS (
                 SELECT 1 FROM information_schema.columns
                 WHERE table_name = 'stream_event' AND column_name = 'scheduled_date'
             ) THEN
                 UPDATE stream_event
                 SET expected_date = scheduled_date
                 WHERE expected_date IS NULL;
             END IF;
         END $$",
    )
    .execute(pool)
    .await?;
    sqlx::query("ALTER TABLE stream_event ALTER COLUMN expected_date SET NOT NULL")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE stream_event DROP COLUMN IF EXISTS scheduled_date")
        .execute(pool)
        .await?;

    sqlx::query(
        "ALTER TABLE stream_schedule
         ALTER COLUMN start_date TYPE DATE
         USING split_part(start_date::text, 'T', 1)::date",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "ALTER TABLE stream_schedule
         ALTER COLUMN end_date TYPE DATE
         USING CASE
             WHEN end_date IS NULL OR end_date::text = '' THEN NULL
             ELSE split_part(end_date::text, 'T', 1)::date
         END",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "ALTER TABLE intg.tmo_import_overview
         ALTER COLUMN snapshot_date TYPE DATE
         USING split_part(snapshot_date::text, 'T', 1)::date",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "ALTER TABLE intg.tmo_import_loan
         ALTER COLUMN maturity_date TYPE DATE
         USING CASE
             WHEN maturity_date IS NULL OR maturity_date::text = '' THEN NULL
             ELSE split_part(maturity_date::text, 'T', 1)::date
         END",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "ALTER TABLE intg.tmo_import_loan
         ALTER COLUMN next_payment_date TYPE DATE
         USING CASE
             WHEN next_payment_date IS NULL OR next_payment_date::text = '' THEN NULL
             ELSE split_part(next_payment_date::text, 'T', 1)::date
         END",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "ALTER TABLE intg.tmo_import_loan
         ALTER COLUMN interest_paid_to TYPE DATE
         USING CASE
             WHEN interest_paid_to IS NULL OR interest_paid_to::text = '' THEN NULL
             ELSE split_part(interest_paid_to::text, 'T', 1)::date
         END",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "ALTER TABLE intg.tmo_import_loan
         ALTER COLUMN billed_through TYPE DATE
         USING CASE
             WHEN billed_through IS NULL OR billed_through::text = '' THEN NULL
             ELSE split_part(billed_through::text, 'T', 1)::date
         END",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "ALTER TABLE intg.tmo_import_payment
         ALTER COLUMN check_date TYPE DATE
         USING split_part(check_date::text, 'T', 1)::date",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "ALTER TABLE portfolio_snapshot
         ALTER COLUMN snapshot_date TYPE DATE
         USING split_part(snapshot_date::text, 'T', 1)::date",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS settings (
            key        TEXT PRIMARY KEY,
            value      TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(pool)
    .await?;

    // Polymorphic link from TMO payments to normalized stream_event rows.
    // Lives in intg so the public schema never references integration tables.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS intg.tmo_payment_event_link (
            tmo_payment_id  BIGINT PRIMARY KEY REFERENCES intg.tmo_import_payment(id) ON DELETE CASCADE,
            stream_event_id BIGINT NOT NULL REFERENCES stream_event(id) ON DELETE CASCADE,
            created_at      TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tmo_payment_event_link_event ON intg.tmo_payment_event_link(stream_event_id)").execute(pool).await?;

    // Backfill link rows from existing normalized events. Idempotent via PK conflict.
    sqlx::query(
        "INSERT INTO intg.tmo_payment_event_link (tmo_payment_id, stream_event_id)
         SELECT p.id, e.id
         FROM intg.tmo_import_payment p
         JOIN stream_event e
           ON e.source_type = 'tmo_history'
          AND e.source_id = p.normalized_event_source_id
         WHERE p.normalized_event_source_id IS NOT NULL
         ON CONFLICT (tmo_payment_id) DO NOTHING",
    )
    .execute(pool)
    .await?;

    // Indexes
    sqlx::query("DROP INDEX IF EXISTS idx_event_stream_scheduled")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_event_stream_expected ON stream_event(stream_id, expected_date)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_event_account ON stream_event(account_id)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_event_expected ON stream_event(expected_date) WHERE expected_date IS NOT NULL").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_event_actual ON stream_event(actual_date) WHERE actual_date IS NOT NULL").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_event_status ON stream_event(status)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_schedule_stream_active ON stream_schedule(stream_id, is_active) WHERE is_active = 1").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_schedule_account ON stream_schedule(account_id)")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_intg_connection_slug ON intg.integration_connection(slug)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_intg_tmo_overview_connection ON intg.tmo_import_overview(connection_id, snapshot_date DESC)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_intg_tmo_loan_connection ON intg.tmo_import_loan(connection_id, loan_account)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_intg_tmo_loan_stream ON intg.tmo_import_loan(stream_id)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_intg_tmo_import_payment_connection ON intg.tmo_import_payment(connection_id, check_date DESC)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_intg_tmo_import_payment_state ON intg.tmo_import_payment(processing_state)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_intg_tmo_credential_account ON intg.tmo_credential(account_number)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_intg_monarch_credential_default_account ON intg.monarch_credential(default_account_id)",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_account_primary ON account(is_primary)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_view_default ON stream_view(is_default)")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_snapshot_date ON portfolio_snapshot(snapshot_date DESC)",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_sync_started ON sync_log(started_at DESC)")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_sync_connection_started ON sync_log(connection_slug, started_at DESC)",
    )
    .execute(pool)
    .await?;

    // -- Received emails (Resend inbound) --
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS intg.received_email (
            id                BIGSERIAL PRIMARY KEY,
            resend_email_id   TEXT NOT NULL UNIQUE,
            from_address      TEXT NOT NULL,
            to_addresses      TEXT NOT NULL,
            subject           TEXT,
            received_at       TEXT NOT NULL,
            body_s3_key       TEXT,
            body_content_type TEXT,
            loan_account      TEXT,
            processing_state  TEXT NOT NULL DEFAULT 'pending',
            error_message     TEXT,
            raw_webhook_payload TEXT,
            created_at        TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at        TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS intg.received_email_attachment (
            id                   BIGSERIAL PRIMARY KEY,
            email_id             BIGINT NOT NULL REFERENCES intg.received_email(id) ON DELETE CASCADE,
            resend_attachment_id TEXT NOT NULL,
            filename             TEXT NOT NULL,
            content_type         TEXT NOT NULL,
            size_bytes           INTEGER,
            s3_key               TEXT,
            processing_state     TEXT NOT NULL DEFAULT 'pending',
            created_at           TEXT NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            UNIQUE(email_id, resend_attachment_id)
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_received_email_loan ON intg.received_email(loan_account) WHERE loan_account IS NOT NULL",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_received_email_unlinked ON intg.received_email(created_at DESC) WHERE loan_account IS NULL",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_received_email_attachment_email ON intg.received_email_attachment(email_id)",
    )
    .execute(pool)
    .await?;

    // -- Application users (session auth) --
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS app_user (
            id            BIGSERIAL PRIMARY KEY,
            email         TEXT    NOT NULL UNIQUE,
            password_hash TEXT    NOT NULL,
            display_name  TEXT,
            is_active     INTEGER NOT NULL DEFAULT 1,
            created_at    TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at    TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
        )",
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// Seed an initial admin user from ADMIN_EMAIL + ADMIN_PASSWORD env vars if no
/// users exist yet. Idempotent: skips if the email already has a row.
pub async fn ensure_admin_user(pool: &PgPool) -> anyhow::Result<()> {
    let existing: Option<(i64,)> = sqlx::query_as("SELECT COUNT(*)::BIGINT FROM app_user")
        .fetch_optional(pool)
        .await?;

    let count = existing.map(|(n,)| n).unwrap_or(0);

    match (config::admin_email(), config::admin_password()) {
        (Some(email), Some(password)) => {
            let hash = crate::auth::hash_password(&password)?;
            let inserted: Option<(i64,)> = sqlx::query_as(
                "INSERT INTO app_user (email, password_hash, display_name)
                 VALUES ($1, $2, 'Admin')
                 ON CONFLICT (email) DO NOTHING
                 RETURNING id",
            )
            .bind(&email)
            .bind(&hash)
            .fetch_optional(pool)
            .await?;

            if inserted.is_some() {
                tracing::info!("seeded admin user {email}");
            }
        }
        _ => {
            if count == 0 {
                tracing::warn!(
                    "no users exist and ADMIN_EMAIL/ADMIN_PASSWORD not set — no one can log in. \
                     Set these env vars and restart to create the initial user."
                );
            }
        }
    }

    Ok(())
}

/// Ensure the "Trustee Income" stream exists, return its id.
pub async fn ensure_trustee_stream(pool: &PgPool) -> anyhow::Result<i64> {
    streams::ensure_default_configuration(pool).await?;
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM stream WHERE type = 'mortgage_portfolio' LIMIT 1")
            .fetch_optional(pool)
            .await?;

    if let Some((id,)) = row {
        return Ok(id);
    }

    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO stream (name, type, description) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind("Trustee Income")
    .bind("mortgage_portfolio")
    .bind("Mortgage loan payments via Val-Chris Investments / The Mortgage Office")
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// Ensure the "Expenses" stream exists, return its id.
pub async fn ensure_expenses_stream(pool: &PgPool) -> anyhow::Result<i64> {
    streams::ensure_default_configuration(pool).await?;
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM stream
             WHERE type IN ('manual_expense', 'expenses')
             ORDER BY id ASC
             LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;

    if let Some((id,)) = row {
        return Ok(id);
    }

    anyhow::bail!("manual expense stream was not created")
}
