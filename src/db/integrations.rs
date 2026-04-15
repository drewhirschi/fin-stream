use sqlx::PgPool;

use crate::models::{
    CapturedProviderRecordView, IntegrationConnectionView, MonarchCredential, PaymentView,
    TmoCredential, TmoImportPaymentView, TmoLoanDetail, TmoLoanSummary, TmoOverview, TmoPayment,
};

fn normalize_date(value: &str) -> &str {
    value.split('T').next().unwrap_or(value)
}

fn normalize_optional_date(value: Option<&str>) -> Option<&str> {
    value.and_then(|value| {
        let trimmed = normalize_date(value).trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

pub async fn ensure_connection(
    pool: &PgPool,
    slug: &str,
    name: &str,
    provider: &str,
    metadata: Option<&str>,
) -> anyhow::Result<i64> {
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO intg.integration_connection (slug, name, provider, metadata)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT(slug) DO UPDATE SET
            name = excluded.name,
            provider = excluded.provider,
            metadata = COALESCE(excluded.metadata, intg.integration_connection.metadata),
            updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
         RETURNING id",
    )
    .bind(slug)
    .bind(name)
    .bind(provider)
    .bind(metadata)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

pub async fn mark_connection_synced(pool: &PgPool, slug: &str) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE intg.integration_connection
         SET status = 'active',
             last_error = NULL,
             last_synced_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
             updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
         WHERE slug = $1",
    )
    .bind(slug)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn mark_connection_error(
    pool: &PgPool,
    slug: &str,
    error_message: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE intg.integration_connection
         SET status = 'error',
             last_error = $2,
             updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
         WHERE slug = $1",
    )
    .bind(slug)
    .bind(error_message)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn update_connection_metadata(
    pool: &PgPool,
    slug: &str,
    metadata: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE intg.integration_connection
         SET metadata = $2,
             updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
         WHERE slug = $1",
    )
    .bind(slug)
    .bind(metadata)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_or_bootstrap_tmo_credential(
    pool: &PgPool,
    connection_id: i64,
) -> anyhow::Result<TmoCredential> {
    let row: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT company_id, account_number, pin_ciphertext, pin_nonce
         FROM intg.tmo_credential
         WHERE connection_id = $1",
    )
    .bind(connection_id)
    .fetch_optional(pool)
    .await?;

    if let Some((company_id, account_number, pin_ciphertext, pin_nonce)) = row {
        return Ok(TmoCredential {
            company_id,
            account_number,
            pin: crate::crypto::decrypt_string(&pin_ciphertext, &pin_nonce)?,
        });
    }

    let company_id = crate::config::tmo_company_id();
    let account_number = crate::config::tmo_account();
    let pin = crate::config::tmo_pin();
    let (pin_ciphertext, pin_nonce) = crate::crypto::encrypt_string(&pin)?;

    sqlx::query(
        "INSERT INTO intg.tmo_credential (
            connection_id, company_id, account_number, pin_ciphertext, pin_nonce, key_version
         )
         VALUES ($1, $2, $3, $4, $5, 1)
         ON CONFLICT (connection_id) DO UPDATE SET
            company_id = excluded.company_id,
            account_number = excluded.account_number,
            pin_ciphertext = excluded.pin_ciphertext,
            pin_nonce = excluded.pin_nonce,
            key_version = excluded.key_version,
            updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
    )
    .bind(connection_id)
    .bind(&company_id)
    .bind(&account_number)
    .bind(&pin_ciphertext)
    .bind(&pin_nonce)
    .execute(pool)
    .await?;

    Ok(TmoCredential {
        company_id,
        account_number,
        pin,
    })
}

pub async fn get_or_bootstrap_monarch_credential(
    pool: &PgPool,
    connection_id: i64,
) -> anyhow::Result<MonarchCredential> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT access_token_ciphertext, access_token_nonce, default_account_id
         FROM intg.monarch_credential
         WHERE connection_id = $1",
    )
    .bind(connection_id)
    .fetch_optional(pool)
    .await?;

    if let Some((access_token_ciphertext, access_token_nonce, default_account_id)) = row {
        return Ok(MonarchCredential {
            access_token: crate::crypto::decrypt_string(
                &access_token_ciphertext,
                &access_token_nonce,
            )?,
            default_account_id,
        });
    }

    let access_token = crate::config::monarch_token();
    let default_account_id = crate::config::monarch_account_id();
    let (access_token_ciphertext, access_token_nonce) =
        crate::crypto::encrypt_string(&access_token)?;

    sqlx::query(
        "INSERT INTO intg.monarch_credential (
            connection_id, access_token_ciphertext, access_token_nonce, default_account_id, key_version
         )
         VALUES ($1, $2, $3, $4, 1)
         ON CONFLICT (connection_id) DO UPDATE SET
            access_token_ciphertext = excluded.access_token_ciphertext,
            access_token_nonce = excluded.access_token_nonce,
            default_account_id = excluded.default_account_id,
            key_version = excluded.key_version,
            updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
    )
    .bind(connection_id)
    .bind(&access_token_ciphertext)
    .bind(&access_token_nonce)
    .bind(&default_account_id)
    .execute(pool)
    .await?;

    Ok(MonarchCredential {
        access_token,
        default_account_id,
    })
}

pub async fn upsert_tmo_import_overview(
    pool: &PgPool,
    connection_id: i64,
    snapshot_date: &str,
    overview: &TmoOverview,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO intg.tmo_import_overview (
            connection_id, snapshot_date, portfolio_value, portfolio_yield, portfolio_count,
            ytd_interest, ytd_principal, trust_balance, outstanding_checks, service_fees,
            raw_payload, processing_state
         )
         VALUES ($1, $2::date, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'captured')
         ON CONFLICT(connection_id, snapshot_date) DO UPDATE SET
            portfolio_value = excluded.portfolio_value,
            portfolio_yield = excluded.portfolio_yield,
            portfolio_count = excluded.portfolio_count,
            ytd_interest = excluded.ytd_interest,
            ytd_principal = excluded.ytd_principal,
            trust_balance = excluded.trust_balance,
            outstanding_checks = excluded.outstanding_checks,
            service_fees = excluded.service_fees,
            raw_payload = excluded.raw_payload,
            updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
    )
    .bind(connection_id)
    .bind(snapshot_date)
    .bind(overview.portfolio_value)
    .bind(overview.portfolio_yield)
    .bind(overview.portfolio_count)
    .bind(overview.ytd_interest)
    .bind(overview.ytd_principal)
    .bind(overview.trust_balance)
    .bind(overview.outstanding_checks_value)
    .bind(overview.ytd_serv_fees)
    .bind(serde_json::to_string(overview)?)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn upsert_tmo_import_loan_summary(
    pool: &PgPool,
    connection_id: i64,
    stream_id: i64,
    loan: &TmoLoanSummary,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO intg.tmo_import_loan (
            connection_id, stream_id, loan_account, borrower_name, property_address, property_city,
            property_state, property_zip, percent_owned, interest_rate, note_rate, maturity_date,
            term_left_months, next_payment_date, interest_paid_to, billed_through, regular_payment,
            loan_balance, principal_balance, is_delinquent, is_active, raw_summary_payload,
            summary_imported_at
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12::date, $13, $14::date, $15::date, $16::date, $17, $18, $19, $20, 1,
                 $21, TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'))
         ON CONFLICT(connection_id, loan_account) DO UPDATE SET
            stream_id = COALESCE(intg.tmo_import_loan.stream_id, excluded.stream_id),
            borrower_name = excluded.borrower_name,
            property_address = excluded.property_address,
            property_city = excluded.property_city,
            property_state = excluded.property_state,
            property_zip = excluded.property_zip,
            percent_owned = excluded.percent_owned,
            interest_rate = excluded.interest_rate,
            note_rate = excluded.note_rate,
            maturity_date = excluded.maturity_date,
            term_left_months = excluded.term_left_months,
            next_payment_date = excluded.next_payment_date,
            interest_paid_to = excluded.interest_paid_to,
            billed_through = excluded.billed_through,
            regular_payment = excluded.regular_payment,
            loan_balance = excluded.loan_balance,
            principal_balance = excluded.principal_balance,
            is_delinquent = excluded.is_delinquent,
            is_active = 1,
            raw_summary_payload = excluded.raw_summary_payload,
            summary_imported_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
            updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
    )
    .bind(connection_id)
    .bind(stream_id)
    .bind(&loan.loan_account)
    .bind(&loan.borrower_name)
    .bind(&loan.primary_street)
    .bind(&loan.primary_city)
    .bind(&loan.primary_state)
    .bind(&loan.primary_zip)
    .bind(loan.percent_owned)
    .bind(loan.interest_rate)
    .bind(loan.interest_rate)
    .bind(normalize_date(&loan.maturity_date))
    .bind(loan.term_left)
    .bind(normalize_date(&loan.next_payment_date))
    .bind(normalize_date(&loan.interest_paid_to_date))
    .bind(normalize_optional_date(loan.billed_through.as_deref()))
    .bind(loan.regular_payment)
    .bind(loan.loan_balance)
    .bind(loan.loan_balance)
    .bind(if loan.is_delinquent { 1 } else { 0 })
    .bind(serde_json::to_string(loan)?)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn upsert_tmo_import_loan_detail(
    pool: &PgPool,
    connection_id: i64,
    detail: &TmoLoanDetail,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE intg.tmo_import_loan
         SET property_description = $3,
             property_type = $4,
             property_priority = $5,
             occupancy = $6,
             ltv = $7,
             appraised_value = $8,
             priority = $9,
             original_balance = $10,
             principal_balance = $11,
             note_rate = $12,
             maturity_date = $13::date,
             next_payment_date = $14::date,
             interest_paid_to = $15::date,
             regular_payment = $16,
             payment_frequency = $17,
             loan_type = $18,
             raw_detail_payload = $19,
             detail_imported_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),
             updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
         WHERE connection_id = $1 AND loan_account = $2",
    )
    .bind(connection_id)
    .bind(&detail.loan_account)
    .bind(&detail.property_description)
    .bind(&detail.property_type)
    .bind(detail.property_priority)
    .bind(&detail.occupancy)
    .bind(detail.ltv)
    .bind(detail.appraised_value)
    .bind(detail.priority)
    .bind(detail.original_balance)
    .bind(detail.principal_balance)
    .bind(detail.note_rate)
    .bind(normalize_date(&detail.maturity_date))
    .bind(normalize_date(&detail.next_payment_date))
    .bind(normalize_date(&detail.interest_paid_to_date))
    .bind(detail.regular_payment)
    .bind(&detail.payment_frequency)
    .bind(detail.loan_type)
    .bind(serde_json::to_string(detail)?)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn replace_tmo_import_payments(
    pool: &PgPool,
    connection_id: i64,
    payments: &[TmoPayment],
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM intg.tmo_import_payment WHERE connection_id = $1")
        .bind(connection_id)
        .execute(&mut *tx)
        .await?;

    for payment in payments {
        let check_date = normalize_date(&payment.check_date);
        let amount_cents = (payment.amount * 100.0).round() as i64;
        let external_id = format!(
            "history:{}:{}:{}",
            payment.loan_account, check_date, amount_cents
        );

        sqlx::query(
            "INSERT INTO intg.tmo_import_payment (
                connection_id, external_id, loan_account, borrower_name, property_name,
                check_number, check_date, amount, service_fee, interest, principal,
                charges, late_charges, other, processing_state, raw_payload
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7::date, $8, $9, $10, $11, $12, $13, $14, 'captured', $15)",
        )
        .bind(connection_id)
        .bind(&external_id)
        .bind(&payment.loan_account)
        .bind(&payment.borrower_name)
        .bind(&payment.property_name)
        .bind(&payment.check_number)
        .bind(check_date)
        .bind(payment.amount)
        .bind(payment.service_fee)
        .bind(payment.interest)
        .bind(payment.principal)
        .bind(payment.charges)
        .bind(payment.late_charges)
        .bind(payment.other)
        .bind(serde_json::to_string(payment)?)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

pub async fn list_tmo_import_payments(
    pool: &PgPool,
    connection_id: i64,
) -> anyhow::Result<Vec<TmoImportPaymentView>> {
    let rows = sqlx::query_as(
        "SELECT id,
                connection_id,
                external_id,
                loan_account,
                borrower_name,
                property_name,
                check_number,
                check_date::text as check_date,
                amount,
                service_fee,
                interest,
                principal,
                charges,
                late_charges,
                other,
                processing_state,
                normalized_event_source_id,
                raw_payload,
                updated_at
         FROM intg.tmo_import_payment
         WHERE connection_id = $1
         ORDER BY check_date DESC, id DESC",
    )
    .bind(connection_id)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn list_recent_tmo_import_payments(
    pool: &PgPool,
    connection_id: i64,
    limit: i32,
) -> Vec<TmoImportPaymentView> {
    sqlx::query_as(
        "SELECT id,
                connection_id,
                external_id,
                loan_account,
                borrower_name,
                property_name,
                check_number,
                check_date::text as check_date,
                amount,
                service_fee,
                interest,
                principal,
                charges,
                late_charges,
                other,
                processing_state,
                normalized_event_source_id,
                raw_payload,
                updated_at
         FROM intg.tmo_import_payment
         WHERE connection_id = $1
         ORDER BY check_date DESC, id DESC
         LIMIT $2",
    )
    .bind(connection_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub async fn list_tmo_import_payments_for_loan(
    pool: &PgPool,
    connection_id: i64,
    loan_account: &str,
    limit: i32,
) -> Vec<TmoImportPaymentView> {
    sqlx::query_as(
        "SELECT id,
                connection_id,
                external_id,
                loan_account,
                borrower_name,
                property_name,
                check_number,
                check_date::text as check_date,
                amount,
                service_fee,
                interest,
                principal,
                charges,
                late_charges,
                other,
                processing_state,
                normalized_event_source_id,
                raw_payload,
                updated_at
         FROM intg.tmo_import_payment
         WHERE connection_id = $1
           AND loan_account = $2
         ORDER BY check_date DESC, id DESC
         LIMIT $3",
    )
    .bind(connection_id)
    .bind(loan_account)
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub async fn mark_payment_normalized(
    pool: &PgPool,
    payment_id: i64,
    normalized_ref: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE intg.tmo_import_payment
         SET processing_state = 'normalized',
             normalized_event_source_id = $2,
             updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
         WHERE id = $1",
    )
    .bind(payment_id)
    .bind(normalized_ref)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn list_connections(pool: &PgPool) -> Vec<IntegrationConnectionView> {
    sqlx::query_as(
        "SELECT c.id,
                c.slug,
                c.name,
                c.provider,
                c.status,
                c.sync_cadence,
                c.last_synced_at,
                c.last_error,
                COALESCE(o.record_count, 0) + COALESCE(l.record_count, 0) + COALESCE(p.record_count, 0) AS record_count,
                COALESCE(p.normalized_count, 0) AS normalized_count,
                COALESCE(p.pending_count, 0) AS pending_count
         FROM intg.integration_connection c
         LEFT JOIN (
             SELECT connection_id, COUNT(*)::bigint AS record_count
             FROM intg.tmo_import_overview
             GROUP BY connection_id
         ) o ON o.connection_id = c.id
         LEFT JOIN (
             SELECT connection_id, COUNT(*)::bigint AS record_count
             FROM intg.tmo_import_loan
             GROUP BY connection_id
         ) l ON l.connection_id = c.id
         LEFT JOIN (
             SELECT connection_id,
                    COUNT(*)::bigint AS record_count,
                    COUNT(*) FILTER (WHERE processing_state = 'normalized')::bigint AS normalized_count,
                    COUNT(*) FILTER (WHERE processing_state <> 'normalized')::bigint AS pending_count
             FROM intg.tmo_import_payment
             GROUP BY connection_id
         ) p ON p.connection_id = c.id
         ORDER BY c.name ASC",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub async fn get_connection_by_slug(
    pool: &PgPool,
    slug: &str,
) -> Option<IntegrationConnectionView> {
    sqlx::query_as(
        "SELECT c.id,
                c.slug,
                c.name,
                c.provider,
                c.status,
                c.sync_cadence,
                c.last_synced_at,
                c.last_error,
                COALESCE(o.record_count, 0) + COALESCE(l.record_count, 0) + COALESCE(p.record_count, 0) AS record_count,
                COALESCE(p.normalized_count, 0) AS normalized_count,
                COALESCE(p.pending_count, 0) AS pending_count
         FROM intg.integration_connection c
         LEFT JOIN (
             SELECT connection_id, COUNT(*)::bigint AS record_count
             FROM intg.tmo_import_overview
             GROUP BY connection_id
         ) o ON o.connection_id = c.id
         LEFT JOIN (
             SELECT connection_id, COUNT(*)::bigint AS record_count
             FROM intg.tmo_import_loan
             GROUP BY connection_id
         ) l ON l.connection_id = c.id
         LEFT JOIN (
             SELECT connection_id,
                    COUNT(*)::bigint AS record_count,
                    COUNT(*) FILTER (WHERE processing_state = 'normalized')::bigint AS normalized_count,
                    COUNT(*) FILTER (WHERE processing_state <> 'normalized')::bigint AS pending_count
             FROM intg.tmo_import_payment
             GROUP BY connection_id
         ) p ON p.connection_id = c.id
         WHERE c.slug = $1",
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

pub async fn list_scheduled_connections(pool: &PgPool) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT slug, sync_cadence
         FROM intg.integration_connection
         WHERE sync_cadence <> 'manual' AND sync_cadence <> ''",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub async fn update_sync_cadence(
    pool: &PgPool,
    slug: &str,
    sync_cadence: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE intg.integration_connection
         SET sync_cadence = $2,
             updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
         WHERE slug = $1",
    )
    .bind(slug)
    .bind(sync_cadence)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn list_sync_logs_for_connection(
    pool: &PgPool,
    slug: &str,
    limit: i32,
) -> Vec<crate::models::SyncLog> {
    sqlx::query_as(
        "SELECT id,
                connection_slug,
                started_at,
                finished_at,
                status,
                error_message,
                endpoints_hit,
                events_upserted,
                loans_upserted,
                snapshots_created
         FROM sync_log
         WHERE connection_slug = $1
         ORDER BY started_at DESC
         LIMIT $2",
    )
    .bind(slug)
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub async fn list_captured_records_for_connection(
    pool: &PgPool,
    connection_id: i64,
    limit: i32,
) -> Vec<CapturedProviderRecordView> {
    sqlx::query_as(
        "SELECT *
         FROM (
             SELECT 'tmo_overview'::text AS entity_type,
                    snapshot_date::text AS external_id,
                    snapshot_date::text AS effective_date,
                    'Portfolio overview snapshot'::text AS summary,
                    trust_balance AS amount,
                    COALESCE(raw_payload, '{}'::text) AS raw_payload,
                    updated_at
             FROM intg.tmo_import_overview
             WHERE connection_id = $1

             UNION ALL

             SELECT 'tmo_loan'::text AS entity_type,
                    loan_account AS external_id,
                    next_payment_date::text AS effective_date,
                    CONCAT(borrower_name, ' - ', COALESCE(property_address, '')) AS summary,
                    regular_payment AS amount,
                    COALESCE(raw_detail_payload, raw_summary_payload, '{}'::text) AS raw_payload,
                    updated_at
             FROM intg.tmo_import_loan
             WHERE connection_id = $1
         ) captured
         ORDER BY effective_date DESC NULLS LAST, updated_at DESC
         LIMIT $2",
    )
    .bind(connection_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub async fn list_normalized_payments(pool: &PgPool, limit: i32) -> Vec<PaymentView> {
    sqlx::query_as(
        "SELECT id,
                label,
                scheduled_date::text as scheduled_date,
                expected_date::text as expected_date,
                actual_date::text as actual_date,
                amount,
                status,
                source_type,
                COALESCE((NULLIF(metadata, '')::jsonb ->> 'is_pending_print_check')::boolean, false) AS is_pending_print_check,
                NULLIF(NULLIF(metadata, '')::jsonb ->> 'check_number', '') AS check_number,
                NULLIF(NULLIF(metadata, '')::jsonb ->> 'loan_account', '') AS loan_account,
                metadata
         FROM stream_event
         WHERE source_type = 'tmo_history'
         ORDER BY COALESCE(actual_date, expected_date, scheduled_date) DESC, id DESC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}
