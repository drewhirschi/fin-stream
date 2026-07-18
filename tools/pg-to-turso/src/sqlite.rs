use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{
    MIGRATION_1, MIGRATION_2, MIGRATION_3, MIGRATION_4, MIGRATION_5,
    convert::{
        argon2id_password_hash, boolean_integer, canonical_timestamp, finite_magnitude,
        finite_number, iso_date, json_array_text, json_text, nonempty, one_of,
    },
    manifest::TableStats,
    model::{
        AccountRow, AppUserRow, Dataset, IntegrationConnectionRow, LoanWorkspacePhotoRow,
        LoanWorkspaceRow, MonarchCredentialRow, PortfolioSnapshotRow, ReceivedEmailAttachmentRow,
        ReceivedEmailRow, SequenceState, SettingRow, StreamEventRow, StreamRow, StreamScheduleRow,
        StreamViewRow, StreamViewStreamRow, SyncLogRow, TmoAccountRow, TmoCredentialRow,
        TmoImportLoanRow, TmoImportOverviewRow, TmoImportPaymentRow, TmoPaymentEventLinkRow,
    },
    stats::dataset_stats,
};

pub struct ArtifactResult {
    pub destination: Dataset,
    pub integrity_check: String,
    pub foreign_key_violations: u64,
    pub artifact_blake3: String,
    pub target_only: Vec<(String, String)>,
}

struct PublishedArtifactGuard<'a> {
    path: &'a Path,
    committed: bool,
}

impl Drop for PublishedArtifactGuard<'_> {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(self.path);
            for suffix in ["-wal", "-shm", "-journal"] {
                let _ = fs::remove_file(format!("{}{suffix}", self.path.display()));
            }
        }
    }
}

pub fn build_artifact(
    output_path: &Path,
    source: &Dataset,
    sequences: &[SequenceState],
) -> Result<ArtifactResult> {
    ensure!(
        !output_path.exists(),
        "refusing to overwrite an existing output file"
    );
    let parent = output_path
        .parent()
        .context("output path has no parent directory")?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".trust-deeds-pg-to-turso-")
        .suffix(".sqlite.partial")
        .tempfile_in(parent)
        .context("could not create private temporary artifact")?;
    set_private_permissions(temporary.path())?;

    let validation = {
        let mut connection = Connection::open(temporary.path())
            .context("could not open temporary SQLite artifact")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA page_size = 4096;\
             PRAGMA auto_vacuum = NONE;\
             PRAGMA encoding = 'UTF-8';",
        )?;
        let journal: String =
            connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        ensure!(
            journal.eq_ignore_ascii_case("wal"),
            "could not enable SQLite WAL during export"
        );
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        let foreign_keys: i64 =
            connection.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
        ensure!(foreign_keys == 1, "SQLite foreign keys are not enabled");
        apply_target_migrations(&mut connection)?;
        load_dataset(&mut connection, source)?;
        validate_sequence_contract(source, sequences)?;
        set_sequences(&connection, sequences)?;
        validate_sequences(&connection, sequences)?;
        validate_foreign_key_enforcement(&connection)?;
        let destination = read_dataset(&connection)?;
        validate_destination_values(&destination)?;

        let source_stats = dataset_stats(source);
        let destination_stats = dataset_stats(&destination);
        ensure_stats_match(&source_stats, &destination_stats)?;
        validate_target_types(&connection)?;
        validate_natural_keys(&connection)?;
        let integrity_check: String =
            connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        ensure!(
            integrity_check == "ok",
            "SQLite integrity_check returned {integrity_check:?}"
        );
        let foreign_key_violations: u64 =
            connection.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })?;
        ensure!(
            foreign_key_violations == 0,
            "SQLite foreign_key_check found violations"
        );
        validate_migration_ledger(&connection)?;
        let sessions: i64 =
            connection.query_row("SELECT COUNT(*) FROM app_session", [], |row| row.get(0))?;
        ensure!(sessions == 0, "target app_session must start empty");
        let inbound_leases: i64 = connection.query_row(
            "SELECT COUNT(*) FROM intg_received_email_processing_lease",
            [],
            |row| row.get(0),
        )?;
        ensure!(
            inbound_leases == 0,
            "target inbound email processing leases must start empty"
        );
        let operation_control: (i64, String, i64, String) = connection.query_row(
            "SELECT id, mode, scheduler_enabled, updated_at FROM operation_control",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        let operation_control_count: i64 =
            connection.query_row("SELECT COUNT(*) FROM operation_control", [], |row| {
                row.get(0)
            })?;
        ensure!(
            operation_control_count == 1
                && operation_control.0 == 1
                && operation_control.1 == "read_only"
                && operation_control.2 == 0,
            "target operation_control must be the inert singleton"
        );
        ensure!(
            canonical_timestamp(&operation_control.3, "operation_control.updated_at")?
                == operation_control.3,
            "operation_control.updated_at is not canonical"
        );

        // Turso's SQLite-file import contract requires WAL to remain the
        // database's persistent journal mode. VACUUM before the final
        // checkpoint so every page is folded into the single main file while
        // the header continues to advertise WAL for the remote import.
        connection.execute_batch("VACUUM;")?;
        let checkpoint: (i64, i64, i64) =
            connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?;
        ensure!(
            checkpoint.0 == 0 && checkpoint.2 == 0,
            "SQLite WAL checkpoint was incomplete"
        );
        validate_turso_import_pragmas(&connection)?;
        connection.close().map_err(|(_, error)| error)?;
        (
            destination,
            integrity_check,
            foreign_key_violations,
            vec![
                (
                    "_schema_migrations".to_owned(),
                    "5 exact ordered name/checksum rows".to_owned(),
                ),
                ("app_session".to_owned(), "0 rows".to_owned()),
                (
                    "intg_received_email_processing_lease".to_owned(),
                    "0 rows".to_owned(),
                ),
                (
                    "operation_control".to_owned(),
                    "1 inert row (id=1, mode=read_only, scheduler_enabled=0)".to_owned(),
                ),
            ],
        )
    };

    temporary.as_file_mut().sync_all()?;
    let persisted = temporary
        .persist_noclobber(output_path)
        .map_err(|error| error.error)
        .context("could not atomically publish SQLite artifact")?;
    let mut published = PublishedArtifactGuard {
        path: output_path,
        committed: false,
    };
    persisted.sync_all()?;
    set_private_permissions(output_path)?;
    sync_parent_directory(parent)?;
    ensure_no_sidecars(output_path)?;
    let artifact_blake3 = artifact_digest(output_path)?;
    published.committed = true;
    Ok(ArtifactResult {
        destination: validation.0,
        integrity_check: validation.1,
        foreign_key_violations: validation.2,
        target_only: validation.3,
        artifact_blake3,
    })
}

fn validate_turso_import_pragmas(connection: &Connection) -> Result<()> {
    let journal_mode: String = connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
    ensure!(
        journal_mode.eq_ignore_ascii_case("wal"),
        "SQLite artifact journal_mode is not WAL"
    );

    let page_size: i64 = connection.query_row("PRAGMA page_size", [], |row| row.get(0))?;
    ensure!(page_size == 4096, "SQLite artifact page_size is not 4096");

    let auto_vacuum: i64 = connection.query_row("PRAGMA auto_vacuum", [], |row| row.get(0))?;
    ensure!(
        auto_vacuum == 0,
        "SQLite artifact auto_vacuum is not disabled"
    );

    let encoding: String = connection.query_row("PRAGMA encoding", [], |row| row.get(0))?;
    ensure!(
        encoding.eq_ignore_ascii_case("utf-8"),
        "SQLite artifact encoding is not UTF-8"
    );
    Ok(())
}

fn apply_target_migrations(connection: &mut Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS _schema_migrations (\
            version INTEGER PRIMARY KEY, \
            name TEXT NOT NULL UNIQUE, \
            checksum TEXT NOT NULL CHECK (length(checksum) = 64), \
            applied_at TEXT NOT NULL \
                DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))\
         ) STRICT;",
    )?;
    for (version, name, sql) in [
        (1_i64, "auth", MIGRATION_1),
        (2_i64, "streams_forecast", MIGRATION_2),
        (3_i64, "integrations_operations", MIGRATION_3),
        (4_i64, "workspaces_inbox", MIGRATION_4),
        (5_i64, "resend_inbound_leases", MIGRATION_5),
    ] {
        let checksum = blake3::hash(sql.as_bytes()).to_hex().to_string();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .with_context(|| format!("begin target migration {version}"))?;
        transaction
            .execute_batch(sql)
            .with_context(|| format!("apply target migration {version} ({name})"))?;
        transaction.execute(
            "INSERT INTO _schema_migrations (version, name, checksum) VALUES (?1, ?2, ?3)",
            params![version, name, checksum],
        )?;
        transaction.commit()?;
    }
    Ok(())
}

fn validate_migration_ledger(connection: &Connection) -> Result<()> {
    let expected = crate::target_migrations();
    let mut statement = connection.prepare(
        "SELECT version, name, checksum, applied_at
         FROM _schema_migrations ORDER BY version",
    )?;
    let records = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        records.len() == expected.len(),
        "target migration ledger has an unexpected row count"
    );
    for ((version, name, checksum, applied_at), expected) in records.iter().zip(expected) {
        ensure!(
            *version == expected.version,
            "target migration ledger version mismatch"
        );
        ensure!(
            name == &expected.name,
            "target migration ledger name mismatch"
        );
        ensure!(
            checksum == &expected.blake3,
            "target migration ledger checksum mismatch"
        );
        canonical_timestamp(applied_at, "_schema_migrations.applied_at")?;
    }
    Ok(())
}

fn load_dataset(connection: &mut Connection, dataset: &Dataset) -> Result<()> {
    let transaction = connection.transaction()?;
    for row in &dataset.app_users {
        transaction.execute(
            "INSERT INTO app_user (id, email, password_hash, display_name, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![row.id, row.email, row.password_hash, row.display_name, row.is_active, row.created_at, row.updated_at],
        )?;
    }
    for row in &dataset.accounts {
        transaction.execute(
            "INSERT INTO account (id, name, kind, balance, balance_as_of_date, source_type,
                 source_ref, metadata, balance_updated_at, is_primary, is_active, notes,
                 created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                row.id,
                row.name,
                row.kind,
                row.balance,
                row.balance_as_of_date,
                row.source_type,
                row.source_ref,
                row.metadata,
                row.balance_updated_at,
                row.is_primary,
                row.is_active,
                row.notes,
                row.created_at,
                row.updated_at
            ],
        )?;
    }
    for row in &dataset.streams {
        transaction.execute(
            "INSERT INTO stream (id, name, type, kind, direction, amount_certainty, description,
                 default_account_id, configuration, parent_id, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                row.id,
                row.name,
                row.stream_type,
                row.kind,
                row.direction,
                row.amount_certainty,
                row.description,
                row.default_account_id,
                row.configuration,
                Option::<i64>::None,
                row.is_active,
                row.created_at,
                row.updated_at
            ],
        )?;
    }
    // A parent may have a larger preserved ID than its child. Restore the
    // self-reference only after every stream exists; SQLite foreign keys are
    // immediate and should not depend on source ID ordering.
    for row in dataset.streams.iter().filter(|row| row.parent_id.is_some()) {
        transaction.execute(
            "UPDATE stream SET parent_id = ?1 WHERE id = ?2",
            params![row.parent_id, row.id],
        )?;
    }
    for row in &dataset.stream_views {
        transaction.execute(
            "INSERT INTO stream_view (id, name, description, is_default, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![row.id, row.name, row.description, row.is_default, row.is_active,
                row.created_at, row.updated_at],
        )?;
    }
    for row in &dataset.stream_view_streams {
        transaction.execute(
            "INSERT INTO stream_view_stream (stream_view_id, stream_id, created_at)
             VALUES (?1, ?2, ?3)",
            params![row.stream_view_id, row.stream_id, row.created_at],
        )?;
    }
    for row in &dataset.stream_schedules {
        transaction.execute(
            "INSERT INTO stream_schedule (id, stream_id, account_id, label, amount, frequency,
                 day_of_month, start_date, end_date, is_active, metadata, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                row.id,
                row.stream_id,
                row.account_id,
                row.label,
                row.amount,
                row.frequency,
                row.day_of_month,
                row.start_date,
                row.end_date,
                row.is_active,
                row.metadata,
                row.created_at,
                row.updated_at
            ],
        )?;
    }
    for row in &dataset.stream_events {
        transaction.execute(
            "INSERT INTO stream_event (id, stream_id, account_id, label, expected_date, amount,
                 override_label, has_label_override, override_date, override_amount,
                 override_account_id, has_account_override, actual_date, actual_amount, status,
                 is_excluded, exclusion_reason, source_id, source_type, metadata, notes,
                 created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                     ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
            params![
                row.id,
                row.stream_id,
                row.account_id,
                row.label,
                row.expected_date,
                row.amount,
                row.override_label,
                row.has_label_override,
                row.override_date,
                row.override_amount,
                row.override_account_id,
                row.has_account_override,
                row.actual_date,
                row.actual_amount,
                row.status,
                row.is_excluded,
                row.exclusion_reason,
                row.source_id,
                row.source_type,
                row.metadata,
                row.notes,
                row.created_at,
                row.updated_at
            ],
        )?;
    }
    for row in &dataset.integration_connections {
        transaction.execute(
            "INSERT INTO intg_integration_connection
                 (id, slug, name, provider, status, sync_cadence, last_synced_at, last_error,
                  metadata, next_scheduled_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                row.id,
                row.slug,
                row.name,
                row.provider,
                row.status,
                row.sync_cadence,
                row.last_synced_at,
                row.last_error,
                row.metadata,
                row.next_scheduled_at,
                row.created_at,
                row.updated_at
            ],
        )?;
    }
    for row in &dataset.tmo_import_overviews {
        transaction.execute(
            "INSERT INTO intg_tmo_import_overview
                 (id, connection_id, snapshot_date, portfolio_value, portfolio_yield,
                  portfolio_count, ytd_interest, ytd_principal, trust_balance,
                  outstanding_checks, service_fees, processing_state, raw_payload,
                  created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                row.id,
                row.connection_id,
                row.snapshot_date,
                row.portfolio_value,
                row.portfolio_yield,
                row.portfolio_count,
                row.ytd_interest,
                row.ytd_principal,
                row.trust_balance,
                row.outstanding_checks,
                row.service_fees,
                row.processing_state,
                row.raw_payload,
                row.created_at,
                row.updated_at
            ],
        )?;
    }
    for row in &dataset.tmo_import_loans {
        transaction.execute(
            "INSERT INTO intg_tmo_import_loan
                 (id, connection_id, stream_id, loan_account, borrower_name, property_address,
                  property_city, property_state, property_zip, property_description,
                  property_type, property_priority, occupancy, appraised_value, ltv,
                  percent_owned, priority, loan_type, interest_rate, note_rate,
                  original_balance, loan_balance, principal_balance, regular_payment,
                  payment_frequency, maturity_date, next_payment_date, interest_paid_to,
                  billed_through, term_left_months, is_delinquent, is_active,
                  raw_summary_payload, raw_detail_payload, summary_imported_at,
                  detail_imported_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                     ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25,
                     ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, ?38)",
            params![
                row.id,
                row.connection_id,
                row.stream_id,
                row.loan_account,
                row.borrower_name,
                row.property_address,
                row.property_city,
                row.property_state,
                row.property_zip,
                row.property_description,
                row.property_type,
                row.property_priority,
                row.occupancy,
                row.appraised_value,
                row.ltv,
                row.percent_owned,
                row.priority,
                row.loan_type,
                row.interest_rate,
                row.note_rate,
                row.original_balance,
                row.loan_balance,
                row.principal_balance,
                row.regular_payment,
                row.payment_frequency,
                row.maturity_date,
                row.next_payment_date,
                row.interest_paid_to,
                row.billed_through,
                row.term_left_months,
                row.is_delinquent,
                row.is_active,
                row.raw_summary_payload,
                row.raw_detail_payload,
                row.summary_imported_at,
                row.detail_imported_at,
                row.created_at,
                row.updated_at
            ],
        )?;
    }
    for row in &dataset.tmo_import_payments {
        transaction.execute(
            "INSERT INTO intg_tmo_import_payment
                 (id, connection_id, external_id, loan_account, borrower_name, property_name,
                  check_number, check_date, amount, service_fee, interest, principal, charges,
                  late_charges, other, processing_state, normalized_event_source_id,
                  raw_payload, imported_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                     ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
            params![
                row.id,
                row.connection_id,
                row.external_id,
                row.loan_account,
                row.borrower_name,
                row.property_name,
                row.check_number,
                row.check_date,
                row.amount,
                row.service_fee,
                row.interest,
                row.principal,
                row.charges,
                row.late_charges,
                row.other,
                row.processing_state,
                row.normalized_event_source_id,
                row.raw_payload,
                row.imported_at,
                row.updated_at
            ],
        )?;
    }
    for row in &dataset.tmo_accounts {
        transaction.execute(
            "INSERT INTO intg_tmo_account
                 (id, company_id, account_number, source_rec_id, display_name, email,
                  last_login_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                row.id,
                row.company_id,
                row.account_number,
                row.source_rec_id,
                row.display_name,
                row.email,
                row.last_login_at,
                row.created_at,
                row.updated_at
            ],
        )?;
    }
    for row in &dataset.tmo_credentials {
        transaction.execute(
            "INSERT INTO intg_tmo_credential
                 (connection_id, company_id, account_number, pin_ciphertext, pin_nonce,
                  key_version, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                row.connection_id,
                row.company_id,
                row.account_number,
                row.pin_ciphertext,
                row.pin_nonce,
                row.key_version,
                row.created_at,
                row.updated_at
            ],
        )?;
    }
    for row in &dataset.monarch_credentials {
        transaction.execute(
            "INSERT INTO intg_monarch_credential
                 (connection_id, access_token_ciphertext, access_token_nonce,
                  default_account_id, key_version, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                row.connection_id,
                row.access_token_ciphertext,
                row.access_token_nonce,
                row.default_account_id,
                row.key_version,
                row.created_at,
                row.updated_at
            ],
        )?;
    }
    for row in &dataset.portfolio_snapshots {
        transaction.execute(
            "INSERT INTO portfolio_snapshot
                 (id, snapshot_date, portfolio_value, portfolio_yield, portfolio_count,
                  ytd_interest, ytd_principal, trust_balance, outstanding_checks,
                  service_fees, synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                row.id,
                row.snapshot_date,
                row.portfolio_value,
                row.portfolio_yield,
                row.portfolio_count,
                row.ytd_interest,
                row.ytd_principal,
                row.trust_balance,
                row.outstanding_checks,
                row.service_fees,
                row.synced_at
            ],
        )?;
    }
    for row in &dataset.settings {
        transaction.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)",
            params![row.key, row.value, row.updated_at],
        )?;
    }
    for row in &dataset.sync_logs {
        transaction.execute(
            "INSERT INTO sync_log
                 (id, connection_slug, scheduled_for, started_at, finished_at, status,
                  error_message, endpoints_hit, events_upserted, loans_upserted,
                  snapshots_created)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                row.id,
                row.connection_slug,
                row.scheduled_for,
                row.started_at,
                row.finished_at,
                row.status,
                row.error_message,
                row.endpoints_hit,
                row.events_upserted,
                row.loans_upserted,
                row.snapshots_created
            ],
        )?;
    }
    for row in &dataset.loan_workspaces {
        transaction.execute(
            "INSERT INTO intg_loan_workspace
                 (id, connection_id, loan_account, redfin_url, zillow_url, decision_status,
                  target_contribution, actual_contribution, notes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                row.id,
                row.connection_id,
                row.loan_account,
                row.redfin_url,
                row.zillow_url,
                row.decision_status,
                row.target_contribution,
                row.actual_contribution,
                row.notes,
                row.created_at,
                row.updated_at
            ],
        )?;
    }
    for row in &dataset.loan_workspace_photos {
        transaction.execute(
            "INSERT INTO intg_loan_workspace_photo
                 (id, connection_id, loan_account, provider, caption, source_url, image_url,
                  sort_order, is_featured, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                row.id,
                row.connection_id,
                row.loan_account,
                row.provider,
                row.caption,
                row.source_url,
                row.image_url,
                row.sort_order,
                row.is_featured,
                row.created_at
            ],
        )?;
    }
    for row in &dataset.received_emails {
        transaction.execute(
            "INSERT INTO intg_received_email
                 (id, resend_email_id, from_address, to_addresses, subject, received_at,
                  body_s3_key, body_content_type, loan_account, processing_state,
                  error_message, raw_webhook_payload, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                row.id,
                row.resend_email_id,
                row.from_address,
                row.to_addresses,
                row.subject,
                row.received_at,
                row.body_s3_key,
                row.body_content_type,
                row.loan_account,
                row.processing_state,
                row.error_message,
                row.raw_webhook_payload,
                row.created_at,
                row.updated_at
            ],
        )?;
    }
    for row in &dataset.received_email_attachments {
        transaction.execute(
            "INSERT INTO intg_received_email_attachment
                 (id, email_id, resend_attachment_id, filename, content_type, size_bytes,
                  s3_key, processing_state, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                row.id,
                row.email_id,
                row.resend_attachment_id,
                row.filename,
                row.content_type,
                row.size_bytes,
                row.s3_key,
                row.processing_state,
                row.created_at
            ],
        )?;
    }
    for row in &dataset.tmo_payment_event_links {
        transaction.execute(
            "INSERT INTO intg_tmo_payment_event_link
                 (tmo_payment_id, stream_event_id, created_at)
             VALUES (?1, ?2, ?3)",
            params![row.tmo_payment_id, row.stream_event_id, row.created_at],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

fn set_sequences(connection: &Connection, sequences: &[SequenceState]) -> Result<()> {
    let transaction = connection.unchecked_transaction()?;
    for state in sequences {
        ensure_known_sequence_table(&state.table)?;
        let target_seq = state.target_effective_next - 1;
        let changed = transaction.execute(
            "UPDATE sqlite_sequence SET seq = ?1 WHERE name = ?2",
            params![target_seq, state.table],
        )?;
        if changed == 0 {
            transaction.execute(
                "INSERT INTO sqlite_sequence (name, seq) VALUES (?1, ?2)",
                params![state.table, target_seq],
            )?;
        }
    }
    transaction.commit()?;
    Ok(())
}

fn validate_sequence_contract(dataset: &Dataset, sequences: &[SequenceState]) -> Result<()> {
    let expected = [
        ("app_user", dataset.app_users.last().map(|row| row.id)),
        ("account", dataset.accounts.last().map(|row| row.id)),
        ("stream", dataset.streams.last().map(|row| row.id)),
        ("stream_view", dataset.stream_views.last().map(|row| row.id)),
        (
            "stream_schedule",
            dataset.stream_schedules.last().map(|row| row.id),
        ),
        (
            "stream_event",
            dataset.stream_events.last().map(|row| row.id),
        ),
        (
            "intg_integration_connection",
            dataset.integration_connections.last().map(|row| row.id),
        ),
        (
            "intg_tmo_import_overview",
            dataset.tmo_import_overviews.last().map(|row| row.id),
        ),
        (
            "intg_tmo_import_loan",
            dataset.tmo_import_loans.last().map(|row| row.id),
        ),
        (
            "intg_tmo_import_payment",
            dataset.tmo_import_payments.last().map(|row| row.id),
        ),
        (
            "portfolio_snapshot",
            dataset.portfolio_snapshots.last().map(|row| row.id),
        ),
        ("sync_log", dataset.sync_logs.last().map(|row| row.id)),
        (
            "intg_loan_workspace",
            dataset.loan_workspaces.last().map(|row| row.id),
        ),
        (
            "intg_loan_workspace_photo",
            dataset.loan_workspace_photos.last().map(|row| row.id),
        ),
        (
            "intg_received_email",
            dataset.received_emails.last().map(|row| row.id),
        ),
        (
            "intg_received_email_attachment",
            dataset.received_email_attachments.last().map(|row| row.id),
        ),
    ];
    ensure!(
        sequences.len() == expected.len(),
        "sequence inventory is incomplete or duplicated"
    );
    for (table, imported_max) in expected {
        let matches: Vec<_> = sequences
            .iter()
            .filter(|state| state.table == table)
            .collect();
        ensure!(
            matches.len() == 1,
            "{table} must have exactly one sequence state"
        );
        let state = matches[0];
        ensure!(
            state.imported_max == imported_max,
            "{table} sequence max differs from imported IDs"
        );
        let minimum_next = imported_max
            .unwrap_or(0)
            .checked_add(1)
            .context("ID overflow")?;
        ensure!(
            state.target_effective_next == state.source_effective_next.max(minimum_next),
            "{table} target sequence does not preserve source next-ID semantics"
        );
    }
    Ok(())
}

fn validate_sequences(connection: &Connection, sequences: &[SequenceState]) -> Result<()> {
    for state in sequences {
        ensure_known_sequence_table(&state.table)?;
        let stored: i64 = connection.query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = ?1",
            [&state.table],
            |row| row.get(0),
        )?;
        ensure!(
            stored == state.target_effective_next - 1,
            "{} sqlite_sequence is wrong",
            state.table
        );
        connection.execute_batch("SAVEPOINT sequence_probe")?;
        let result = probe_next_id(connection, &state.table, state.target_effective_next);
        connection.execute_batch("ROLLBACK TO sequence_probe; RELEASE sequence_probe")?;
        let actual = result?;
        ensure!(
            actual == state.target_effective_next,
            "{} generated {actual}, expected {}",
            state.table,
            state.target_effective_next
        );
        let after: i64 = connection.query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = ?1",
            [&state.table],
            |row| row.get(0),
        )?;
        ensure!(
            after == stored,
            "{} sequence probe was not rolled back",
            state.table
        );
    }
    Ok(())
}

fn validate_foreign_key_enforcement(connection: &Connection) -> Result<()> {
    let maximum: i64 =
        connection.query_row("SELECT COALESCE(MAX(id), 0) FROM stream", [], |row| {
            row.get(0)
        })?;
    let missing_stream_id = maximum
        .checked_add(1)
        .context("cannot construct foreign-key validation probe")?;
    connection.execute_batch("SAVEPOINT foreign_key_probe")?;
    let result = connection.execute(
        "INSERT INTO stream_event (stream_id, expected_date, amount, status)
         VALUES (?1, '2000-01-01', 0.0, 'projected')",
        [missing_stream_id],
    );
    connection.execute_batch("ROLLBACK TO foreign_key_probe; RELEASE foreign_key_probe")?;
    match result {
        Err(rusqlite::Error::SqliteFailure(error, _))
            if error.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY =>
        {
            Ok(())
        }
        Err(error) => Err(error).context("foreign-key probe failed for an unexpected reason"),
        Ok(_) => bail!("SQLite accepted an invalid foreign-key write"),
    }
}

fn probe_next_id(connection: &Connection, table: &str, expected: i64) -> Result<i64> {
    let marker = format!("sequence-probe-{table}-{expected}");
    match table {
        "app_user" => {
            connection.execute(
                "INSERT INTO app_user (email, password_hash, display_name, is_active)
                 VALUES (?1, 'not-a-real-hash', 'Sequence probe', 0)",
                [format!("{marker}@invalid.example")],
            )?;
        }
        "account" => {
            connection.execute(
                "INSERT INTO account (name, kind, is_primary, is_active) VALUES (?1, 'probe', 0, 0)",
                [&marker],
            )?;
        }
        "stream" => {
            connection.execute(
                "INSERT INTO stream (name, type, kind, direction, amount_certainty, is_active)
                 VALUES (?1, 'probe', 'probe', 'in', 'known', 0)",
                [&marker],
            )?;
        }
        "stream_view" => {
            connection.execute(
                "INSERT INTO stream_view (name, is_default, is_active) VALUES (?1, 0, 0)",
                [&marker],
            )?;
        }
        "stream_schedule" | "stream_event" => {
            let parent_id: i64 =
                connection.query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM stream", [], |row| {
                    row.get(0)
                })?;
            connection.execute(
                "INSERT INTO stream (id, name, type, kind, direction, amount_certainty, is_active)
                 VALUES (?1, ?2, 'probe', 'probe', 'in', 'known', 0)",
                params![parent_id, marker],
            )?;
            if table == "stream_schedule" {
                connection.execute(
                    "INSERT INTO stream_schedule (stream_id, amount, frequency, start_date, is_active)
                     VALUES (?1, 0.0, 'one_time', '2000-01-01', 0)",
                    [parent_id],
                )?;
            } else {
                connection.execute(
                    "INSERT INTO stream_event (stream_id, expected_date, amount, status)
                     VALUES (?1, '2000-01-01', 0.0, 'projected')",
                    [parent_id],
                )?;
            }
        }
        "intg_integration_connection" => {
            connection.execute(
                "INSERT INTO intg_integration_connection
                     (slug, name, provider, status, sync_cadence)
                 VALUES (?1, ?2, 'probe', 'error', 'manual')",
                params![marker, "Sequence probe"],
            )?;
        }
        "intg_tmo_import_overview" => {
            let connection_id = insert_probe_connection(connection, &marker)?;
            connection.execute(
                "INSERT INTO intg_tmo_import_overview
                     (connection_id, snapshot_date, processing_state)
                 VALUES (?1, '2000-01-01', 'captured')",
                [connection_id],
            )?;
        }
        "intg_tmo_import_loan" => {
            let connection_id = insert_probe_connection(connection, &marker)?;
            connection.execute(
                "INSERT INTO intg_tmo_import_loan
                     (connection_id, loan_account, is_delinquent, is_active)
                 VALUES (?1, ?2, 0, 0)",
                params![connection_id, marker],
            )?;
        }
        "intg_tmo_import_payment" => {
            let connection_id = insert_probe_connection(connection, &marker)?;
            connection.execute(
                "INSERT INTO intg_tmo_import_payment
                     (connection_id, external_id, loan_account, borrower_name, property_name,
                      check_date, amount, service_fee, interest, principal, charges,
                      late_charges, other, processing_state)
                 VALUES (?1, ?2, ?2, 'Sequence probe', 'Sequence probe', '2000-01-01',
                         0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 'captured')",
                params![connection_id, marker],
            )?;
        }
        "portfolio_snapshot" => {
            let date = unused_probe_date(connection)?;
            connection.execute(
                "INSERT INTO portfolio_snapshot (snapshot_date) VALUES (?1)",
                [date],
            )?;
        }
        "sync_log" => {
            connection.execute(
                "INSERT INTO sync_log
                     (connection_slug, started_at, finished_at, status,
                      events_upserted, loans_upserted, snapshots_created)
                 VALUES (?1, '2000-01-01T00:00:00.000Z',
                         '2000-01-01T00:00:00.000Z', 'success', 0, 0, 0)",
                [marker],
            )?;
        }
        "intg_loan_workspace" => {
            let connection_id = insert_probe_connection(connection, &marker)?;
            connection.execute(
                "INSERT INTO intg_loan_workspace (connection_id, loan_account)
                 VALUES (?1, ?2)",
                params![connection_id, marker],
            )?;
        }
        "intg_loan_workspace_photo" => {
            let connection_id = insert_probe_connection(connection, &marker)?;
            connection.execute(
                "INSERT INTO intg_loan_workspace_photo
                     (connection_id, loan_account, provider, source_url, image_url,
                      sort_order, is_featured)
                 VALUES (?1, ?2, 'probe', ?3, ?4, 0, 0)",
                params![
                    connection_id,
                    marker,
                    format!("https://invalid.example/{marker}/source"),
                    format!("https://invalid.example/{marker}/image")
                ],
            )?;
        }
        "intg_received_email" => {
            connection.execute(
                "INSERT INTO intg_received_email
                     (resend_email_id, from_address, to_addresses, received_at,
                      processing_state)
                 VALUES (?1, 'probe@invalid.example', '[]',
                         '2000-01-01T00:00:00.000Z', 'pending')",
                [marker],
            )?;
        }
        "intg_received_email_attachment" => {
            let email_id: i64 = connection.query_row(
                "SELECT COALESCE(MAX(id), 0) + 1 FROM intg_received_email",
                [],
                |row| row.get(0),
            )?;
            connection.execute(
                "INSERT INTO intg_received_email
                     (id, resend_email_id, from_address, to_addresses, received_at,
                      processing_state)
                 VALUES (?1, ?2, 'probe@invalid.example', '[]',
                         '2000-01-01T00:00:00.000Z', 'pending')",
                params![email_id, format!("{marker}-email")],
            )?;
            connection.execute(
                "INSERT INTO intg_received_email_attachment
                     (email_id, resend_attachment_id, filename, content_type,
                      processing_state)
                 VALUES (?1, ?2, 'probe.bin', 'application/octet-stream', 'pending')",
                params![email_id, marker],
            )?;
        }
        _ => bail!("unsupported sequence probe table {table}"),
    }
    Ok(connection.last_insert_rowid())
}

fn insert_probe_connection(connection: &Connection, marker: &str) -> Result<i64> {
    let id: i64 = connection.query_row(
        "SELECT COALESCE(MAX(id), 0) + 1 FROM intg_integration_connection",
        [],
        |row| row.get(0),
    )?;
    connection.execute(
        "INSERT INTO intg_integration_connection
             (id, slug, name, provider, status, sync_cadence)
         VALUES (?1, ?2, 'Sequence probe', 'probe', 'error', 'manual')",
        params![id, format!("{marker}-connection")],
    )?;
    Ok(id)
}

fn unused_probe_date(connection: &Connection) -> Result<String> {
    for year in 1..=9999 {
        let candidate = format!("{year:04}-01-01");
        let exists: i64 = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM portfolio_snapshot WHERE snapshot_date = ?1)",
            [&candidate],
            |row| row.get(0),
        )?;
        if exists == 0 {
            return Ok(candidate);
        }
    }
    bail!("could not find an unused portfolio snapshot date for sequence validation")
}

fn ensure_known_sequence_table(table: &str) -> Result<()> {
    if matches!(
        table,
        "app_user"
            | "account"
            | "stream"
            | "stream_view"
            | "stream_schedule"
            | "stream_event"
            | "intg_integration_connection"
            | "intg_tmo_import_overview"
            | "intg_tmo_import_loan"
            | "intg_tmo_import_payment"
            | "portfolio_snapshot"
            | "sync_log"
            | "intg_loan_workspace"
            | "intg_loan_workspace_photo"
            | "intg_received_email"
            | "intg_received_email_attachment"
    ) {
        Ok(())
    } else {
        bail!("refusing dynamic sequence operation for unknown table {table}")
    }
}

fn read_dataset(connection: &Connection) -> Result<Dataset> {
    Ok(Dataset {
        app_users: query_rows(
            connection,
            "SELECT id, email, password_hash, display_name, is_active, created_at, updated_at FROM app_user ORDER BY id",
            |row| {
                Ok(AppUserRow {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    password_hash: row.get(2)?,
                    display_name: row.get(3)?,
                    is_active: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            },
        )?,
        accounts: query_rows(
            connection,
            "SELECT id, name, kind, balance, balance_as_of_date, source_type, source_ref, metadata, balance_updated_at, is_primary, is_active, notes, created_at, updated_at FROM account ORDER BY id",
            |row| {
                Ok(AccountRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    kind: row.get(2)?,
                    balance: row.get(3)?,
                    balance_as_of_date: row.get(4)?,
                    source_type: row.get(5)?,
                    source_ref: row.get(6)?,
                    metadata: row.get(7)?,
                    balance_updated_at: row.get(8)?,
                    is_primary: row.get(9)?,
                    is_active: row.get(10)?,
                    notes: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                })
            },
        )?,
        streams: query_rows(
            connection,
            "SELECT id, name, type, kind, direction, amount_certainty, description, default_account_id, configuration, parent_id, is_active, created_at, updated_at FROM stream ORDER BY id",
            |row| {
                Ok(StreamRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    stream_type: row.get(2)?,
                    kind: row.get(3)?,
                    direction: row.get(4)?,
                    amount_certainty: row.get(5)?,
                    description: row.get(6)?,
                    default_account_id: row.get(7)?,
                    configuration: row.get(8)?,
                    parent_id: row.get(9)?,
                    is_active: row.get(10)?,
                    created_at: row.get(11)?,
                    updated_at: row.get(12)?,
                })
            },
        )?,
        stream_views: query_rows(
            connection,
            "SELECT id, name, description, is_default, is_active, created_at, updated_at FROM stream_view ORDER BY id",
            |row| {
                Ok(StreamViewRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    is_default: row.get(3)?,
                    is_active: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            },
        )?,
        stream_view_streams: query_rows(
            connection,
            "SELECT stream_view_id, stream_id, created_at FROM stream_view_stream ORDER BY stream_view_id, stream_id",
            |row| {
                Ok(StreamViewStreamRow {
                    stream_view_id: row.get(0)?,
                    stream_id: row.get(1)?,
                    created_at: row.get(2)?,
                })
            },
        )?,
        stream_schedules: query_rows(
            connection,
            "SELECT id, stream_id, account_id, label, amount, frequency, day_of_month, start_date, end_date, is_active, metadata, created_at, updated_at FROM stream_schedule ORDER BY id",
            |row| {
                Ok(StreamScheduleRow {
                    id: row.get(0)?,
                    stream_id: row.get(1)?,
                    account_id: row.get(2)?,
                    label: row.get(3)?,
                    amount: row.get(4)?,
                    frequency: row.get(5)?,
                    day_of_month: row.get(6)?,
                    start_date: row.get(7)?,
                    end_date: row.get(8)?,
                    is_active: row.get(9)?,
                    metadata: row.get(10)?,
                    created_at: row.get(11)?,
                    updated_at: row.get(12)?,
                })
            },
        )?,
        stream_events: query_rows(
            connection,
            "SELECT id, stream_id, account_id, label, expected_date, amount, override_label, has_label_override, override_date, override_amount, override_account_id, has_account_override, actual_date, actual_amount, status, is_excluded, exclusion_reason, source_id, source_type, metadata, notes, created_at, updated_at FROM stream_event ORDER BY id",
            |row| {
                Ok(StreamEventRow {
                    id: row.get(0)?,
                    stream_id: row.get(1)?,
                    account_id: row.get(2)?,
                    label: row.get(3)?,
                    expected_date: row.get(4)?,
                    amount: row.get(5)?,
                    override_label: row.get(6)?,
                    has_label_override: row.get(7)?,
                    override_date: row.get(8)?,
                    override_amount: row.get(9)?,
                    override_account_id: row.get(10)?,
                    has_account_override: row.get(11)?,
                    actual_date: row.get(12)?,
                    actual_amount: row.get(13)?,
                    status: row.get(14)?,
                    is_excluded: row.get(15)?,
                    exclusion_reason: row.get(16)?,
                    source_id: row.get(17)?,
                    source_type: row.get(18)?,
                    metadata: row.get(19)?,
                    notes: row.get(20)?,
                    created_at: row.get(21)?,
                    updated_at: row.get(22)?,
                })
            },
        )?,
        integration_connections: query_rows(
            connection,
            "SELECT id, slug, name, provider, status, sync_cadence, last_synced_at,
                    last_error, metadata, next_scheduled_at, created_at, updated_at
             FROM intg_integration_connection ORDER BY id",
            |row| {
                Ok(IntegrationConnectionRow {
                    id: row.get(0)?,
                    slug: row.get(1)?,
                    name: row.get(2)?,
                    provider: row.get(3)?,
                    status: row.get(4)?,
                    sync_cadence: row.get(5)?,
                    last_synced_at: row.get(6)?,
                    last_error: row.get(7)?,
                    metadata: row.get(8)?,
                    next_scheduled_at: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                })
            },
        )?,
        tmo_import_overviews: query_rows(
            connection,
            "SELECT id, connection_id, snapshot_date, portfolio_value, portfolio_yield,
                    portfolio_count, ytd_interest, ytd_principal, trust_balance,
                    outstanding_checks, service_fees, processing_state, raw_payload,
                    created_at, updated_at
             FROM intg_tmo_import_overview ORDER BY id",
            |row| {
                Ok(TmoImportOverviewRow {
                    id: row.get(0)?,
                    connection_id: row.get(1)?,
                    snapshot_date: row.get(2)?,
                    portfolio_value: row.get(3)?,
                    portfolio_yield: row.get(4)?,
                    portfolio_count: row.get(5)?,
                    ytd_interest: row.get(6)?,
                    ytd_principal: row.get(7)?,
                    trust_balance: row.get(8)?,
                    outstanding_checks: row.get(9)?,
                    service_fees: row.get(10)?,
                    processing_state: row.get(11)?,
                    raw_payload: row.get(12)?,
                    created_at: row.get(13)?,
                    updated_at: row.get(14)?,
                })
            },
        )?,
        tmo_import_loans: query_rows(
            connection,
            "SELECT id, connection_id, stream_id, loan_account, borrower_name,
                    property_address, property_city, property_state, property_zip,
                    property_description, property_type, property_priority, occupancy,
                    appraised_value, ltv, percent_owned, priority, loan_type, interest_rate,
                    note_rate, original_balance, loan_balance, principal_balance,
                    regular_payment, payment_frequency, maturity_date, next_payment_date,
                    interest_paid_to, billed_through, term_left_months, is_delinquent,
                    is_active, raw_summary_payload, raw_detail_payload, summary_imported_at,
                    detail_imported_at, created_at, updated_at
             FROM intg_tmo_import_loan ORDER BY id",
            |row| {
                Ok(TmoImportLoanRow {
                    id: row.get(0)?,
                    connection_id: row.get(1)?,
                    stream_id: row.get(2)?,
                    loan_account: row.get(3)?,
                    borrower_name: row.get(4)?,
                    property_address: row.get(5)?,
                    property_city: row.get(6)?,
                    property_state: row.get(7)?,
                    property_zip: row.get(8)?,
                    property_description: row.get(9)?,
                    property_type: row.get(10)?,
                    property_priority: row.get(11)?,
                    occupancy: row.get(12)?,
                    appraised_value: row.get(13)?,
                    ltv: row.get(14)?,
                    percent_owned: row.get(15)?,
                    priority: row.get(16)?,
                    loan_type: row.get(17)?,
                    interest_rate: row.get(18)?,
                    note_rate: row.get(19)?,
                    original_balance: row.get(20)?,
                    loan_balance: row.get(21)?,
                    principal_balance: row.get(22)?,
                    regular_payment: row.get(23)?,
                    payment_frequency: row.get(24)?,
                    maturity_date: row.get(25)?,
                    next_payment_date: row.get(26)?,
                    interest_paid_to: row.get(27)?,
                    billed_through: row.get(28)?,
                    term_left_months: row.get(29)?,
                    is_delinquent: row.get(30)?,
                    is_active: row.get(31)?,
                    raw_summary_payload: row.get(32)?,
                    raw_detail_payload: row.get(33)?,
                    summary_imported_at: row.get(34)?,
                    detail_imported_at: row.get(35)?,
                    created_at: row.get(36)?,
                    updated_at: row.get(37)?,
                })
            },
        )?,
        tmo_import_payments: query_rows(
            connection,
            "SELECT id, connection_id, external_id, loan_account, borrower_name,
                    property_name, check_number, check_date, amount, service_fee, interest,
                    principal, charges, late_charges, other, processing_state,
                    normalized_event_source_id, raw_payload, imported_at, updated_at
             FROM intg_tmo_import_payment ORDER BY id",
            |row| {
                Ok(TmoImportPaymentRow {
                    id: row.get(0)?,
                    connection_id: row.get(1)?,
                    external_id: row.get(2)?,
                    loan_account: row.get(3)?,
                    borrower_name: row.get(4)?,
                    property_name: row.get(5)?,
                    check_number: row.get(6)?,
                    check_date: row.get(7)?,
                    amount: row.get(8)?,
                    service_fee: row.get(9)?,
                    interest: row.get(10)?,
                    principal: row.get(11)?,
                    charges: row.get(12)?,
                    late_charges: row.get(13)?,
                    other: row.get(14)?,
                    processing_state: row.get(15)?,
                    normalized_event_source_id: row.get(16)?,
                    raw_payload: row.get(17)?,
                    imported_at: row.get(18)?,
                    updated_at: row.get(19)?,
                })
            },
        )?,
        tmo_accounts: query_rows(
            connection,
            "SELECT id, company_id, account_number, source_rec_id, display_name, email,
                    last_login_at, created_at, updated_at
             FROM intg_tmo_account ORDER BY id",
            |row| {
                Ok(TmoAccountRow {
                    id: row.get(0)?,
                    company_id: row.get(1)?,
                    account_number: row.get(2)?,
                    source_rec_id: row.get(3)?,
                    display_name: row.get(4)?,
                    email: row.get(5)?,
                    last_login_at: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            },
        )?,
        tmo_credentials: query_rows(
            connection,
            "SELECT connection_id, company_id, account_number, pin_ciphertext, pin_nonce,
                    key_version, created_at, updated_at
             FROM intg_tmo_credential ORDER BY connection_id",
            |row| {
                Ok(TmoCredentialRow {
                    connection_id: row.get(0)?,
                    company_id: row.get(1)?,
                    account_number: row.get(2)?,
                    pin_ciphertext: row.get(3)?,
                    pin_nonce: row.get(4)?,
                    key_version: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        )?,
        monarch_credentials: query_rows(
            connection,
            "SELECT connection_id, access_token_ciphertext, access_token_nonce,
                    default_account_id, key_version, created_at, updated_at
             FROM intg_monarch_credential ORDER BY connection_id",
            |row| {
                Ok(MonarchCredentialRow {
                    connection_id: row.get(0)?,
                    access_token_ciphertext: row.get(1)?,
                    access_token_nonce: row.get(2)?,
                    default_account_id: row.get(3)?,
                    key_version: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            },
        )?,
        tmo_payment_event_links: query_rows(
            connection,
            "SELECT tmo_payment_id, stream_event_id, created_at
             FROM intg_tmo_payment_event_link ORDER BY tmo_payment_id",
            |row| {
                Ok(TmoPaymentEventLinkRow {
                    tmo_payment_id: row.get(0)?,
                    stream_event_id: row.get(1)?,
                    created_at: row.get(2)?,
                })
            },
        )?,
        portfolio_snapshots: query_rows(
            connection,
            "SELECT id, snapshot_date, portfolio_value, portfolio_yield, portfolio_count,
                    ytd_interest, ytd_principal, trust_balance, outstanding_checks,
                    service_fees, synced_at
             FROM portfolio_snapshot ORDER BY id",
            |row| {
                Ok(PortfolioSnapshotRow {
                    id: row.get(0)?,
                    snapshot_date: row.get(1)?,
                    portfolio_value: row.get(2)?,
                    portfolio_yield: row.get(3)?,
                    portfolio_count: row.get(4)?,
                    ytd_interest: row.get(5)?,
                    ytd_principal: row.get(6)?,
                    trust_balance: row.get(7)?,
                    outstanding_checks: row.get(8)?,
                    service_fees: row.get(9)?,
                    synced_at: row.get(10)?,
                })
            },
        )?,
        settings: query_rows(
            connection,
            "SELECT key, value, updated_at FROM settings ORDER BY key",
            |row| {
                Ok(SettingRow {
                    key: row.get(0)?,
                    value: row.get(1)?,
                    updated_at: row.get(2)?,
                })
            },
        )?,
        sync_logs: query_rows(
            connection,
            "SELECT id, connection_slug, scheduled_for, started_at, finished_at, status,
                    error_message, endpoints_hit, events_upserted, loans_upserted,
                    snapshots_created
             FROM sync_log ORDER BY id",
            |row| {
                Ok(SyncLogRow {
                    id: row.get(0)?,
                    connection_slug: row.get(1)?,
                    scheduled_for: row.get(2)?,
                    started_at: row.get(3)?,
                    finished_at: row.get(4)?,
                    status: row.get(5)?,
                    error_message: row.get(6)?,
                    endpoints_hit: row.get(7)?,
                    events_upserted: row.get(8)?,
                    loans_upserted: row.get(9)?,
                    snapshots_created: row.get(10)?,
                })
            },
        )?,
        loan_workspaces: query_rows(
            connection,
            "SELECT id, connection_id, loan_account, redfin_url, zillow_url,
                    decision_status, target_contribution, actual_contribution, notes,
                    created_at, updated_at
             FROM intg_loan_workspace ORDER BY id",
            |row| {
                Ok(LoanWorkspaceRow {
                    id: row.get(0)?,
                    connection_id: row.get(1)?,
                    loan_account: row.get(2)?,
                    redfin_url: row.get(3)?,
                    zillow_url: row.get(4)?,
                    decision_status: row.get(5)?,
                    target_contribution: row.get(6)?,
                    actual_contribution: row.get(7)?,
                    notes: row.get(8)?,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                })
            },
        )?,
        loan_workspace_photos: query_rows(
            connection,
            "SELECT id, connection_id, loan_account, provider, caption, source_url,
                    image_url, sort_order, is_featured, created_at
             FROM intg_loan_workspace_photo ORDER BY id",
            |row| {
                Ok(LoanWorkspacePhotoRow {
                    id: row.get(0)?,
                    connection_id: row.get(1)?,
                    loan_account: row.get(2)?,
                    provider: row.get(3)?,
                    caption: row.get(4)?,
                    source_url: row.get(5)?,
                    image_url: row.get(6)?,
                    sort_order: row.get(7)?,
                    is_featured: row.get(8)?,
                    created_at: row.get(9)?,
                })
            },
        )?,
        received_emails: query_rows(
            connection,
            "SELECT id, resend_email_id, from_address, to_addresses, subject, received_at,
                    body_s3_key, body_content_type, loan_account, processing_state,
                    error_message, raw_webhook_payload, created_at, updated_at
             FROM intg_received_email ORDER BY id",
            |row| {
                Ok(ReceivedEmailRow {
                    id: row.get(0)?,
                    resend_email_id: row.get(1)?,
                    from_address: row.get(2)?,
                    to_addresses: row.get(3)?,
                    subject: row.get(4)?,
                    received_at: row.get(5)?,
                    body_s3_key: row.get(6)?,
                    body_content_type: row.get(7)?,
                    loan_account: row.get(8)?,
                    processing_state: row.get(9)?,
                    error_message: row.get(10)?,
                    raw_webhook_payload: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                })
            },
        )?,
        received_email_attachments: query_rows(
            connection,
            "SELECT id, email_id, resend_attachment_id, filename, content_type, size_bytes,
                    s3_key, processing_state, created_at
             FROM intg_received_email_attachment ORDER BY id",
            |row| {
                Ok(ReceivedEmailAttachmentRow {
                    id: row.get(0)?,
                    email_id: row.get(1)?,
                    resend_attachment_id: row.get(2)?,
                    filename: row.get(3)?,
                    content_type: row.get(4)?,
                    size_bytes: row.get(5)?,
                    s3_key: row.get(6)?,
                    processing_state: row.get(7)?,
                    created_at: row.get(8)?,
                })
            },
        )?,
    })
}

fn validate_destination_values(dataset: &Dataset) -> Result<()> {
    let mut active_users = 0_u64;
    for row in &dataset.app_users {
        nonempty(row.email.clone(), &format!("app_user[{}].email", row.id))?;
        let password_hash = nonempty(
            row.password_hash.clone(),
            &format!("app_user[{}].password_hash", row.id),
        )?;
        boolean_integer(row.is_active, &format!("app_user[{}].is_active", row.id))?;
        if row.is_active == 1 {
            active_users += 1;
            argon2id_password_hash(
                password_hash,
                &format!("app_user[{}].password_hash", row.id),
            )?;
        }
    }
    ensure!(active_users > 0, "destination has no active login user");
    for row in &dataset.accounts {
        nonempty(row.name.clone(), &format!("account[{}].name", row.id))?;
        nonempty(row.kind.clone(), &format!("account[{}].kind", row.id))?;
        if let Some(balance) = row.balance {
            finite_number(balance, &format!("account[{}].balance", row.id))?;
        }
        match (&row.balance, &row.balance_as_of_date) {
            (None, None) | (Some(_), Some(_)) => {}
            _ => bail!("account[{}] balance/as-of presence differs", row.id),
        }
        check_date(
            row.balance_as_of_date.as_deref(),
            &format!("account[{}].balance_as_of_date", row.id),
        )?;
        check_json(
            row.metadata.as_deref(),
            &format!("account[{}].metadata", row.id),
        )?;
        check_timestamp(
            row.balance_updated_at.as_deref(),
            &format!("account[{}].balance_updated_at", row.id),
        )?;
        boolean_integer(row.is_primary, &format!("account[{}].is_primary", row.id))?;
        boolean_integer(row.is_active, &format!("account[{}].is_active", row.id))?;
        check_timestamp(
            Some(&row.created_at),
            &format!("account[{}].created_at", row.id),
        )?;
        check_timestamp(
            Some(&row.updated_at),
            &format!("account[{}].updated_at", row.id),
        )?;
    }
    for row in &dataset.streams {
        nonempty(row.name.clone(), &format!("stream[{}].name", row.id))?;
        nonempty(row.stream_type.clone(), &format!("stream[{}].type", row.id))?;
        nonempty(row.kind.clone(), &format!("stream[{}].kind", row.id))?;
        one_of(
            row.direction.clone(),
            &["in", "out"],
            &format!("stream[{}].direction", row.id),
        )?;
        one_of(
            row.amount_certainty.clone(),
            &["known", "estimated"],
            &format!("stream[{}].amount_certainty", row.id),
        )?;
        check_json(
            row.configuration.as_deref(),
            &format!("stream[{}].configuration", row.id),
        )?;
        boolean_integer(row.is_active, &format!("stream[{}].is_active", row.id))?;
        check_timestamp(
            Some(&row.created_at),
            &format!("stream[{}].created_at", row.id),
        )?;
        check_timestamp(
            Some(&row.updated_at),
            &format!("stream[{}].updated_at", row.id),
        )?;
    }
    for row in &dataset.stream_views {
        nonempty(row.name.clone(), &format!("stream_view[{}].name", row.id))?;
        boolean_integer(
            row.is_default,
            &format!("stream_view[{}].is_default", row.id),
        )?;
        boolean_integer(row.is_active, &format!("stream_view[{}].is_active", row.id))?;
        check_timestamp(
            Some(&row.created_at),
            &format!("stream_view[{}].created_at", row.id),
        )?;
        check_timestamp(
            Some(&row.updated_at),
            &format!("stream_view[{}].updated_at", row.id),
        )?;
    }
    for row in &dataset.stream_view_streams {
        check_timestamp(
            Some(&row.created_at),
            &format!(
                "stream_view_stream[{},{}].created_at",
                row.stream_view_id, row.stream_id
            ),
        )?;
    }
    for row in &dataset.stream_schedules {
        finite_magnitude(row.amount, &format!("stream_schedule[{}].amount", row.id))?;
        one_of(
            row.frequency.clone(),
            &[
                "monthly",
                "semimonthly",
                "biweekly",
                "weekly",
                "annual",
                "one_time",
            ],
            &format!("stream_schedule[{}].frequency", row.id),
        )?;
        if let Some(day) = row.day_of_month {
            ensure!(
                (1..=31).contains(&day),
                "stream_schedule[{}].day_of_month is invalid",
                row.id
            );
        }
        check_date(
            Some(&row.start_date),
            &format!("stream_schedule[{}].start_date", row.id),
        )?;
        check_date(
            row.end_date.as_deref(),
            &format!("stream_schedule[{}].end_date", row.id),
        )?;
        boolean_integer(
            row.is_active,
            &format!("stream_schedule[{}].is_active", row.id),
        )?;
        check_json(
            row.metadata.as_deref(),
            &format!("stream_schedule[{}].metadata", row.id),
        )?;
        check_timestamp(
            Some(&row.created_at),
            &format!("stream_schedule[{}].created_at", row.id),
        )?;
        check_timestamp(
            Some(&row.updated_at),
            &format!("stream_schedule[{}].updated_at", row.id),
        )?;
    }
    for row in &dataset.stream_events {
        check_date(
            Some(&row.expected_date),
            &format!("stream_event[{}].expected_date", row.id),
        )?;
        finite_magnitude(row.amount, &format!("stream_event[{}].amount", row.id))?;
        check_date(
            row.override_date.as_deref(),
            &format!("stream_event[{}].override_date", row.id),
        )?;
        if let Some(amount) = row.override_amount {
            finite_magnitude(amount, &format!("stream_event[{}].override_amount", row.id))?;
        }
        boolean_integer(
            row.has_label_override,
            &format!("stream_event[{}].has_label_override", row.id),
        )?;
        boolean_integer(
            row.has_account_override,
            &format!("stream_event[{}].has_account_override", row.id),
        )?;
        check_date(
            row.actual_date.as_deref(),
            &format!("stream_event[{}].actual_date", row.id),
        )?;
        if let Some(amount) = row.actual_amount {
            finite_magnitude(amount, &format!("stream_event[{}].actual_amount", row.id))?;
        }
        one_of(
            row.status.clone(),
            &["projected", "confirmed", "received"],
            &format!("stream_event[{}].status", row.id),
        )?;
        boolean_integer(
            row.is_excluded,
            &format!("stream_event[{}].is_excluded", row.id),
        )?;
        if row.status == "received" {
            ensure!(
                row.actual_date.is_some() && row.actual_amount.is_some(),
                "stream_event[{}] received state is incomplete",
                row.id
            );
        }
        check_json(
            row.metadata.as_deref(),
            &format!("stream_event[{}].metadata", row.id),
        )?;
        check_timestamp(
            Some(&row.created_at),
            &format!("stream_event[{}].created_at", row.id),
        )?;
        check_timestamp(
            Some(&row.updated_at),
            &format!("stream_event[{}].updated_at", row.id),
        )?;
    }
    validate_integrations_destination_values(dataset)?;
    Ok(())
}

fn validate_integrations_destination_values(dataset: &Dataset) -> Result<()> {
    for row in &dataset.integration_connections {
        let field = |name: &str| format!("integration_connection[{}].{name}", row.id);
        nonempty(row.slug.clone(), &field("slug"))?;
        nonempty(row.name.clone(), &field("name"))?;
        nonempty(row.provider.clone(), &field("provider"))?;
        one_of(row.status.clone(), &["active", "error"], &field("status"))?;
        nonempty(row.sync_cadence.clone(), &field("sync_cadence"))?;
        check_timestamp(row.last_synced_at.as_deref(), &field("last_synced_at"))?;
        check_json(row.metadata.as_deref(), &field("metadata"))?;
        check_timestamp(
            row.next_scheduled_at.as_deref(),
            &field("next_scheduled_at"),
        )?;
        check_timestamp(Some(&row.created_at), &field("created_at"))?;
        check_timestamp(Some(&row.updated_at), &field("updated_at"))?;
    }
    for row in &dataset.tmo_import_overviews {
        let field = |name: &str| format!("tmo_import_overview[{}].{name}", row.id);
        check_date(Some(&row.snapshot_date), &field("snapshot_date"))?;
        for (name, value) in [
            ("portfolio_value", row.portfolio_value),
            ("portfolio_yield", row.portfolio_yield),
            ("ytd_interest", row.ytd_interest),
            ("ytd_principal", row.ytd_principal),
            ("trust_balance", row.trust_balance),
            ("outstanding_checks", row.outstanding_checks),
            ("service_fees", row.service_fees),
        ] {
            check_optional_finite(value, &field(name))?;
        }
        one_of(
            row.processing_state.clone(),
            &["captured"],
            &field("processing_state"),
        )?;
        check_json(row.raw_payload.as_deref(), &field("raw_payload"))?;
        check_timestamp(Some(&row.created_at), &field("created_at"))?;
        check_timestamp(Some(&row.updated_at), &field("updated_at"))?;
    }
    for row in &dataset.tmo_import_loans {
        let field = |name: &str| format!("tmo_import_loan[{}].{name}", row.id);
        nonempty(row.loan_account.clone(), &field("loan_account"))?;
        for (name, value) in [
            ("appraised_value", row.appraised_value),
            ("ltv", row.ltv),
            ("percent_owned", row.percent_owned),
            ("interest_rate", row.interest_rate),
            ("note_rate", row.note_rate),
            ("original_balance", row.original_balance),
            ("loan_balance", row.loan_balance),
            ("principal_balance", row.principal_balance),
            ("regular_payment", row.regular_payment),
        ] {
            check_optional_finite(value, &field(name))?;
        }
        check_date(row.maturity_date.as_deref(), &field("maturity_date"))?;
        check_date(
            row.next_payment_date.as_deref(),
            &field("next_payment_date"),
        )?;
        check_date(row.interest_paid_to.as_deref(), &field("interest_paid_to"))?;
        check_date(row.billed_through.as_deref(), &field("billed_through"))?;
        if let Some(value) = row.is_delinquent {
            boolean_integer(value, &field("is_delinquent"))?;
        }
        if let Some(value) = row.is_active {
            boolean_integer(value, &field("is_active"))?;
        }
        check_json(
            row.raw_summary_payload.as_deref(),
            &field("raw_summary_payload"),
        )?;
        check_json(
            row.raw_detail_payload.as_deref(),
            &field("raw_detail_payload"),
        )?;
        check_timestamp(
            row.summary_imported_at.as_deref(),
            &field("summary_imported_at"),
        )?;
        check_timestamp(
            row.detail_imported_at.as_deref(),
            &field("detail_imported_at"),
        )?;
        check_timestamp(Some(&row.created_at), &field("created_at"))?;
        check_timestamp(Some(&row.updated_at), &field("updated_at"))?;
    }
    for row in &dataset.tmo_import_payments {
        let field = |name: &str| format!("tmo_import_payment[{}].{name}", row.id);
        nonempty(row.external_id.clone(), &field("external_id"))?;
        nonempty(row.loan_account.clone(), &field("loan_account"))?;
        nonempty(row.borrower_name.clone(), &field("borrower_name"))?;
        nonempty(row.property_name.clone(), &field("property_name"))?;
        check_date(Some(&row.check_date), &field("check_date"))?;
        for (name, value) in [
            ("amount", row.amount),
            ("service_fee", row.service_fee),
            ("interest", row.interest),
            ("principal", row.principal),
            ("charges", row.charges),
            ("late_charges", row.late_charges),
            ("other", row.other),
        ] {
            finite_number(value, &field(name))?;
        }
        one_of(
            row.processing_state.clone(),
            &["captured", "normalized"],
            &field("processing_state"),
        )?;
        check_json(row.raw_payload.as_deref(), &field("raw_payload"))?;
        check_timestamp(Some(&row.imported_at), &field("imported_at"))?;
        check_timestamp(Some(&row.updated_at), &field("updated_at"))?;
    }
    ensure!(
        dataset.tmo_accounts.len() <= 1,
        "destination has more than one tmo_account row"
    );
    for row in &dataset.tmo_accounts {
        ensure!(row.id == 1, "tmo_account id must be 1");
        nonempty(row.company_id.clone(), "tmo_account.company_id")?;
        nonempty(row.account_number.clone(), "tmo_account.account_number")?;
        check_timestamp(row.last_login_at.as_deref(), "tmo_account.last_login_at")?;
        check_timestamp(Some(&row.created_at), "tmo_account.created_at")?;
        check_timestamp(Some(&row.updated_at), "tmo_account.updated_at")?;
    }
    for row in &dataset.tmo_credentials {
        let field = |name: &str| format!("tmo_credential[{}].{name}", row.connection_id);
        nonempty(row.company_id.clone(), &field("company_id"))?;
        nonempty(row.account_number.clone(), &field("account_number"))?;
        nonempty(row.pin_ciphertext.clone(), &field("pin_ciphertext"))?;
        nonempty(row.pin_nonce.clone(), &field("pin_nonce"))?;
        ensure!(
            row.key_version > 0,
            "{} is not positive",
            field("key_version")
        );
        check_timestamp(Some(&row.created_at), &field("created_at"))?;
        check_timestamp(Some(&row.updated_at), &field("updated_at"))?;
    }
    for row in &dataset.monarch_credentials {
        let field = |name: &str| format!("monarch_credential[{}].{name}", row.connection_id);
        nonempty(
            row.access_token_ciphertext.clone(),
            &field("access_token_ciphertext"),
        )?;
        nonempty(row.access_token_nonce.clone(), &field("access_token_nonce"))?;
        nonempty(row.default_account_id.clone(), &field("default_account_id"))?;
        ensure!(
            row.key_version > 0,
            "{} is not positive",
            field("key_version")
        );
        check_timestamp(Some(&row.created_at), &field("created_at"))?;
        check_timestamp(Some(&row.updated_at), &field("updated_at"))?;
    }
    for row in &dataset.tmo_payment_event_links {
        check_timestamp(
            Some(&row.created_at),
            &format!("tmo_payment_event_link[{}].created_at", row.tmo_payment_id),
        )?;
    }
    for row in &dataset.portfolio_snapshots {
        let field = |name: &str| format!("portfolio_snapshot[{}].{name}", row.id);
        check_date(Some(&row.snapshot_date), &field("snapshot_date"))?;
        for (name, value) in [
            ("portfolio_value", row.portfolio_value),
            ("portfolio_yield", row.portfolio_yield),
            ("ytd_interest", row.ytd_interest),
            ("ytd_principal", row.ytd_principal),
            ("trust_balance", row.trust_balance),
            ("outstanding_checks", row.outstanding_checks),
            ("service_fees", row.service_fees),
        ] {
            check_optional_finite(value, &field(name))?;
        }
        check_timestamp(Some(&row.synced_at), &field("synced_at"))?;
    }
    for row in &dataset.settings {
        nonempty(row.key.clone(), "settings.key")?;
        check_timestamp(
            Some(&row.updated_at),
            &format!("settings[{}].updated_at", row.key),
        )?;
    }
    for row in &dataset.sync_logs {
        let field = |name: &str| format!("sync_log[{}].{name}", row.id);
        nonempty(row.connection_slug.clone(), &field("connection_slug"))?;
        check_timestamp(row.scheduled_for.as_deref(), &field("scheduled_for"))?;
        check_timestamp(Some(&row.started_at), &field("started_at"))?;
        let finished_at = row
            .finished_at
            .as_deref()
            .context("destination sync_log contains a terminal execution without finished_at")?;
        check_timestamp(Some(finished_at), &field("finished_at"))?;
        one_of(row.status.clone(), &["success", "error"], &field("status"))?;
        ensure!(
            finished_at >= row.started_at.as_str(),
            "{} precedes started_at",
            field("finished_at")
        );
        for (name, value) in [
            ("events_upserted", row.events_upserted),
            ("loans_upserted", row.loans_upserted),
            ("snapshots_created", row.snapshots_created),
        ] {
            ensure!(value >= 0, "{} is negative", field(name));
        }
    }
    for row in &dataset.loan_workspaces {
        let field = |name: &str| format!("loan_workspace[{}].{name}", row.id);
        nonempty(row.loan_account.clone(), &field("loan_account"))?;
        if let Some(status) = &row.decision_status {
            one_of(
                status.clone(),
                &["new", "reviewing", "committed", "funded", "passed"],
                &field("decision_status"),
            )?;
        }
        check_optional_finite(row.target_contribution, &field("target_contribution"))?;
        check_optional_finite(row.actual_contribution, &field("actual_contribution"))?;
        check_timestamp(Some(&row.created_at), &field("created_at"))?;
        check_timestamp(Some(&row.updated_at), &field("updated_at"))?;
    }
    for row in &dataset.loan_workspace_photos {
        let field = |name: &str| format!("loan_workspace_photo[{}].{name}", row.id);
        nonempty(row.loan_account.clone(), &field("loan_account"))?;
        nonempty(row.provider.clone(), &field("provider"))?;
        nonempty(row.source_url.clone(), &field("source_url"))?;
        nonempty(row.image_url.clone(), &field("image_url"))?;
        boolean_integer(row.is_featured, &field("is_featured"))?;
        check_timestamp(Some(&row.created_at), &field("created_at"))?;
    }
    for row in &dataset.received_emails {
        let field = |name: &str| format!("received_email[{}].{name}", row.id);
        nonempty(row.resend_email_id.clone(), &field("resend_email_id"))?;
        nonempty(row.from_address.clone(), &field("from_address"))?;
        json_array_text(row.to_addresses.clone(), &field("to_addresses"))?;
        check_timestamp(Some(&row.received_at), &field("received_at"))?;
        if let Some(account) = &row.loan_account {
            nonempty(account.clone(), &field("loan_account"))?;
        }
        one_of(
            row.processing_state.clone(),
            &["pending", "stored", "error"],
            &field("processing_state"),
        )?;
        check_json(
            row.raw_webhook_payload.as_deref(),
            &field("raw_webhook_payload"),
        )?;
        check_timestamp(Some(&row.created_at), &field("created_at"))?;
        check_timestamp(Some(&row.updated_at), &field("updated_at"))?;
    }
    for row in &dataset.received_email_attachments {
        let field = |name: &str| format!("received_email_attachment[{}].{name}", row.id);
        nonempty(
            row.resend_attachment_id.clone(),
            &field("resend_attachment_id"),
        )?;
        nonempty(row.filename.clone(), &field("filename"))?;
        nonempty(row.content_type.clone(), &field("content_type"))?;
        ensure!(
            row.size_bytes.is_none_or(|size| size >= 0),
            "{} is negative",
            field("size_bytes")
        );
        one_of(
            row.processing_state.clone(),
            &["pending", "stored", "error"],
            &field("processing_state"),
        )?;
        check_timestamp(Some(&row.created_at), &field("created_at"))?;
    }
    Ok(())
}

fn check_optional_finite(value: Option<f64>, field: &str) -> Result<()> {
    if let Some(value) = value {
        finite_number(value, field)?;
    }
    Ok(())
}

fn check_date(value: Option<&str>, field: &str) -> Result<()> {
    if let Some(value) = value {
        ensure!(iso_date(value, field)? == value, "{field} is not canonical");
    }
    Ok(())
}

fn check_timestamp(value: Option<&str>, field: &str) -> Result<()> {
    if let Some(value) = value {
        ensure!(
            canonical_timestamp(value, field)? == value,
            "{field} is not canonical"
        );
    }
    Ok(())
}

fn check_json(value: Option<&str>, field: &str) -> Result<()> {
    json_text(value.map(str::to_owned), field)?;
    Ok(())
}

fn query_rows<T>(
    connection: &Connection,
    sql: &str,
    map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>> {
    let mut statement = connection.prepare(sql)?;
    statement
        .query_map([], map)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn ensure_stats_match(
    source: &std::collections::BTreeMap<&str, TableStats>,
    destination: &std::collections::BTreeMap<&str, TableStats>,
) -> Result<()> {
    ensure!(
        source.keys().eq(destination.keys()),
        "source/destination table sets differ"
    );
    for (table, source_stats) in source {
        let destination_stats = &destination[table];
        ensure!(
            source_stats == destination_stats,
            "{table} source/destination count, key range, digest, or financial totals differ"
        );
    }
    Ok(())
}

fn validate_target_types(connection: &Connection) -> Result<()> {
    let checks = [
        (
            "app_user",
            "typeof(id) <> 'integer' OR typeof(email) <> 'text' OR typeof(password_hash) <> 'text' OR typeof(is_active) <> 'integer' OR typeof(created_at) <> 'integer' OR typeof(updated_at) <> 'integer'",
        ),
        (
            "account",
            "typeof(id) <> 'integer' OR typeof(name) <> 'text' OR typeof(kind) <> 'text' OR (balance IS NOT NULL AND typeof(balance) <> 'real') OR typeof(is_primary) <> 'integer' OR typeof(is_active) <> 'integer'",
        ),
        (
            "stream",
            "typeof(id) <> 'integer' OR typeof(name) <> 'text' OR typeof(type) <> 'text' OR typeof(kind) <> 'text' OR typeof(direction) <> 'text' OR typeof(amount_certainty) <> 'text' OR typeof(is_active) <> 'integer'",
        ),
        (
            "stream_view",
            "typeof(id) <> 'integer' OR typeof(name) <> 'text' OR typeof(is_default) <> 'integer' OR typeof(is_active) <> 'integer'",
        ),
        (
            "stream_view_stream",
            "typeof(stream_view_id) <> 'integer' OR typeof(stream_id) <> 'integer' OR typeof(created_at) <> 'text'",
        ),
        (
            "stream_schedule",
            "typeof(id) <> 'integer' OR typeof(stream_id) <> 'integer' OR typeof(amount) <> 'real' OR typeof(frequency) <> 'text' OR typeof(start_date) <> 'text' OR typeof(is_active) <> 'integer'",
        ),
        (
            "stream_event",
            "typeof(id) <> 'integer' OR typeof(stream_id) <> 'integer' OR typeof(expected_date) <> 'text' OR typeof(amount) <> 'real' OR typeof(status) <> 'text' OR typeof(is_excluded) <> 'integer'",
        ),
        (
            "intg_integration_connection",
            "typeof(id) <> 'integer' OR typeof(slug) <> 'text' OR typeof(name) <> 'text' OR typeof(provider) <> 'text' OR typeof(status) <> 'text' OR typeof(sync_cadence) <> 'text' OR typeof(created_at) <> 'text' OR typeof(updated_at) <> 'text'",
        ),
        (
            "intg_tmo_import_overview",
            "typeof(id) <> 'integer' OR typeof(connection_id) <> 'integer' OR typeof(snapshot_date) <> 'text' OR (portfolio_value IS NOT NULL AND typeof(portfolio_value) <> 'real') OR (portfolio_count IS NOT NULL AND typeof(portfolio_count) <> 'integer') OR typeof(processing_state) <> 'text' OR typeof(created_at) <> 'text' OR typeof(updated_at) <> 'text'",
        ),
        (
            "intg_tmo_import_loan",
            "typeof(id) <> 'integer' OR typeof(connection_id) <> 'integer' OR (stream_id IS NOT NULL AND typeof(stream_id) <> 'integer') OR typeof(loan_account) <> 'text' OR (appraised_value IS NOT NULL AND typeof(appraised_value) <> 'real') OR (is_delinquent IS NOT NULL AND typeof(is_delinquent) <> 'integer') OR (is_active IS NOT NULL AND typeof(is_active) <> 'integer') OR typeof(created_at) <> 'text' OR typeof(updated_at) <> 'text'",
        ),
        (
            "intg_tmo_import_payment",
            "typeof(id) <> 'integer' OR typeof(connection_id) <> 'integer' OR typeof(external_id) <> 'text' OR typeof(loan_account) <> 'text' OR typeof(check_date) <> 'text' OR typeof(amount) <> 'real' OR typeof(service_fee) <> 'real' OR typeof(interest) <> 'real' OR typeof(principal) <> 'real' OR typeof(charges) <> 'real' OR typeof(late_charges) <> 'real' OR typeof(other) <> 'real' OR typeof(processing_state) <> 'text' OR typeof(imported_at) <> 'text' OR typeof(updated_at) <> 'text'",
        ),
        (
            "intg_tmo_account",
            "typeof(id) <> 'integer' OR typeof(company_id) <> 'text' OR typeof(account_number) <> 'text' OR typeof(created_at) <> 'text' OR typeof(updated_at) <> 'text'",
        ),
        (
            "intg_tmo_credential",
            "typeof(connection_id) <> 'integer' OR typeof(company_id) <> 'text' OR typeof(account_number) <> 'text' OR typeof(pin_ciphertext) <> 'text' OR typeof(pin_nonce) <> 'text' OR typeof(key_version) <> 'integer' OR typeof(created_at) <> 'text' OR typeof(updated_at) <> 'text'",
        ),
        (
            "intg_monarch_credential",
            "typeof(connection_id) <> 'integer' OR typeof(access_token_ciphertext) <> 'text' OR typeof(access_token_nonce) <> 'text' OR typeof(default_account_id) <> 'text' OR typeof(key_version) <> 'integer' OR typeof(created_at) <> 'text' OR typeof(updated_at) <> 'text'",
        ),
        (
            "intg_tmo_payment_event_link",
            "typeof(tmo_payment_id) <> 'integer' OR typeof(stream_event_id) <> 'integer' OR typeof(created_at) <> 'text'",
        ),
        (
            "portfolio_snapshot",
            "typeof(id) <> 'integer' OR typeof(snapshot_date) <> 'text' OR (portfolio_value IS NOT NULL AND typeof(portfolio_value) <> 'real') OR (portfolio_count IS NOT NULL AND typeof(portfolio_count) <> 'integer') OR typeof(synced_at) <> 'text'",
        ),
        (
            "settings",
            "typeof(key) <> 'text' OR typeof(value) <> 'text' OR typeof(updated_at) <> 'text'",
        ),
        (
            "sync_log",
            "typeof(id) <> 'integer' OR typeof(connection_slug) <> 'text' OR typeof(started_at) <> 'text' OR typeof(status) <> 'text' OR typeof(events_upserted) <> 'integer' OR typeof(loans_upserted) <> 'integer' OR typeof(snapshots_created) <> 'integer'",
        ),
        (
            "intg_loan_workspace",
            "typeof(id) <> 'integer' OR typeof(connection_id) <> 'integer' OR typeof(loan_account) <> 'text' OR (target_contribution IS NOT NULL AND typeof(target_contribution) <> 'real') OR (actual_contribution IS NOT NULL AND typeof(actual_contribution) <> 'real') OR typeof(created_at) <> 'text' OR typeof(updated_at) <> 'text'",
        ),
        (
            "intg_loan_workspace_photo",
            "typeof(id) <> 'integer' OR typeof(connection_id) <> 'integer' OR typeof(loan_account) <> 'text' OR typeof(provider) <> 'text' OR typeof(source_url) <> 'text' OR typeof(image_url) <> 'text' OR typeof(sort_order) <> 'integer' OR typeof(is_featured) <> 'integer' OR typeof(created_at) <> 'text'",
        ),
        (
            "intg_received_email",
            "typeof(id) <> 'integer' OR typeof(resend_email_id) <> 'text' OR typeof(from_address) <> 'text' OR typeof(to_addresses) <> 'text' OR typeof(received_at) <> 'text' OR typeof(processing_state) <> 'text' OR typeof(created_at) <> 'text' OR typeof(updated_at) <> 'text'",
        ),
        (
            "intg_received_email_attachment",
            "typeof(id) <> 'integer' OR typeof(email_id) <> 'integer' OR typeof(resend_attachment_id) <> 'text' OR typeof(filename) <> 'text' OR typeof(content_type) <> 'text' OR (size_bytes IS NOT NULL AND typeof(size_bytes) <> 'integer') OR typeof(processing_state) <> 'text' OR typeof(created_at) <> 'text'",
        ),
    ];
    for (table, predicate) in checks {
        let query = format!("SELECT COUNT(*) FROM {table} WHERE {predicate}");
        let invalid: i64 = connection.query_row(&query, [], |row| row.get(0))?;
        ensure!(
            invalid == 0,
            "{table} contains values with unexpected SQLite storage classes"
        );
    }
    Ok(())
}

fn validate_natural_keys(connection: &Connection) -> Result<()> {
    let duplicate_emails: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM app_user GROUP BY lower(email) HAVING COUNT(*) > 1 LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    ensure!(
        duplicate_emails.is_none(),
        "case-insensitive app_user email duplicate"
    );
    let duplicate_events: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM stream_event
             WHERE source_type IS NOT NULL AND source_id IS NOT NULL
             GROUP BY stream_id, source_type, source_id HAVING COUNT(*) > 1 LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    ensure!(
        duplicate_events.is_none(),
        "stream_event natural-key duplicate"
    );
    let duplicate_checks = [
        (
            "integration connection slug",
            "SELECT 1 FROM intg_integration_connection GROUP BY slug HAVING COUNT(*) > 1 LIMIT 1",
        ),
        (
            "TMO overview snapshot",
            "SELECT 1 FROM intg_tmo_import_overview GROUP BY connection_id, snapshot_date HAVING COUNT(*) > 1 LIMIT 1",
        ),
        (
            "TMO loan account",
            "SELECT 1 FROM intg_tmo_import_loan GROUP BY connection_id, loan_account HAVING COUNT(*) > 1 LIMIT 1",
        ),
        (
            "TMO payment external ID",
            "SELECT 1 FROM intg_tmo_import_payment GROUP BY connection_id, external_id HAVING COUNT(*) > 1 LIMIT 1",
        ),
        (
            "portfolio snapshot date",
            "SELECT 1 FROM portfolio_snapshot GROUP BY snapshot_date HAVING COUNT(*) > 1 LIMIT 1",
        ),
        (
            "running sync execution",
            "SELECT 1 FROM sync_log WHERE status = 'running' GROUP BY connection_slug HAVING COUNT(*) > 1 LIMIT 1",
        ),
        (
            "scheduled sync slot",
            "SELECT 1 FROM sync_log WHERE scheduled_for IS NOT NULL GROUP BY connection_slug, scheduled_for HAVING COUNT(*) > 1 LIMIT 1",
        ),
        (
            "loan workspace",
            "SELECT 1 FROM intg_loan_workspace GROUP BY connection_id, loan_account HAVING COUNT(*) > 1 LIMIT 1",
        ),
        (
            "loan workspace photo",
            "SELECT 1 FROM intg_loan_workspace_photo GROUP BY connection_id, loan_account, provider, image_url HAVING COUNT(*) > 1 LIMIT 1",
        ),
        (
            "featured loan workspace photo",
            "SELECT 1 FROM intg_loan_workspace_photo WHERE is_featured = 1 GROUP BY connection_id, loan_account HAVING COUNT(*) > 1 LIMIT 1",
        ),
        (
            "received email external ID",
            "SELECT 1 FROM intg_received_email GROUP BY resend_email_id HAVING COUNT(*) > 1 LIMIT 1",
        ),
        (
            "received email attachment external ID",
            "SELECT 1 FROM intg_received_email_attachment GROUP BY email_id, resend_attachment_id HAVING COUNT(*) > 1 LIMIT 1",
        ),
    ];
    for (label, query) in duplicate_checks {
        let duplicate: Option<i64> = connection
            .query_row(query, [], |row| row.get(0))
            .optional()?;
        ensure!(duplicate.is_none(), "{label} natural-key duplicate");
    }
    Ok(())
}

fn ensure_no_sidecars(path: &Path) -> Result<()> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        ensure!(!sidecar.exists(), "SQLite sidecar remains after checkpoint");
    }
    Ok(())
}

fn artifact_digest(path: &Path) -> Result<String> {
    use std::io::Read;

    let mut file = fs::File::open(path)?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut hasher = blake3::Hasher::new();
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    let mode = fs::metadata(path)?.permissions().mode() & 0o777;
    ensure!(mode == 0o600, "artifact mode is {mode:o}, expected 600");
    Ok(())
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> Result<()> {
    bail!("the exporter currently requires Unix mode-0600 file semantics")
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<()> {
    fs::File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> Result<()> {
    Ok(())
}
