use sqlx::PgPool;

const TMO_CONNECTION_SLUG: &str = "tmo";

/// Run a full sync: login, fetch overview, portfolio, loan details, payment history.
/// Returns a SyncLog-like summary.
pub async fn run_full_sync(pool: &PgPool) -> anyhow::Result<SyncSummary> {
    let now = chrono::Utc::now().to_rfc3339();

    // Insert sync_log row as "running"
    let (log_id,): (i64,) = sqlx::query_as(
        "INSERT INTO sync_log (connection_slug, started_at, status) VALUES ($1, $2, 'running') RETURNING id",
    )
    .bind(TMO_CONNECTION_SLUG)
    .bind(&now)
    .fetch_one(pool)
    .await?;

    let result = run_sync_inner(pool).await;

    let finished = chrono::Utc::now().to_rfc3339();

    match &result {
        Ok(summary) => {
            sqlx::query(
                "UPDATE sync_log SET finished_at = $1, status = 'success',
                 endpoints_hit = $2, events_upserted = $3, loans_upserted = $4, snapshots_created = $5
                 WHERE id = $6",
            )
            .bind(&finished)
            .bind(&summary.endpoints_hit)
            .bind(summary.events_upserted)
            .bind(summary.loans_upserted)
            .bind(summary.snapshots_created)
            .bind(log_id)
            .execute(pool)
            .await?;
        }
        Err(e) => {
            let _ = crate::db::integrations::mark_connection_error(
                pool,
                TMO_CONNECTION_SLUG,
                &e.to_string(),
            )
            .await;
            sqlx::query(
                "UPDATE sync_log SET finished_at = $1, status = 'error', error_message = $2 WHERE id = $3",
            )
            .bind(&finished)
            .bind(e.to_string())
            .bind(log_id)
            .execute(pool)
            .await?;
        }
    }

    result
}

pub struct SyncSummary {
    pub endpoints_hit: String,
    pub loans_upserted: i32,
    pub events_upserted: i32,
    pub snapshots_created: i32,
}

async fn run_sync_inner(pool: &PgPool) -> anyhow::Result<SyncSummary> {
    let stream_id = crate::db::ensure_trustee_stream(pool).await?;
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let connection_id = crate::db::integrations::ensure_connection(
        pool,
        TMO_CONNECTION_SLUG,
        "The Mortgage Office",
        "mortgage_office",
        None,
    )
    .await?;
    let credential =
        crate::db::integrations::get_or_bootstrap_tmo_credential(pool, connection_id).await?;
    let connection_metadata = serde_json::json!({
        "company_id": credential.company_id,
        "account": credential.account_number,
    })
    .to_string();
    crate::db::integrations::update_connection_metadata(
        pool,
        TMO_CONNECTION_SLUG,
        &connection_metadata,
    )
    .await?;
    let client = crate::tmo::client::TmoClient::login(
        &credential.company_id,
        &credential.account_number,
        &credential.pin,
    )
    .await?;

    let mut endpoints = Vec::new();
    let mut loans_upserted = 0;
    let mut events_upserted = 0;
    let mut snapshots_created = 0;

    // Clean up stale projected events and remove legacy TMO payment projections.
    let stale_count = crate::db::events::cleanup_stale_projections(pool).await?;
    if stale_count > 0 {
        tracing::info!("cleaned up {} stale projected events", stale_count);
    }
    let removed_tmo_projections = crate::db::events::cleanup_tmo_projections(pool).await?;
    if removed_tmo_projections > 0 {
        tracing::info!(
            "removed {} legacy TMO projected payment events",
            removed_tmo_projections
        );
    }

    // 1. Overview → portfolio_snapshot
    tracing::info!("syncing overview...");
    let overview = client.get_overview().await?;
    endpoints.push("overview");
    crate::db::integrations::upsert_tmo_import_overview(pool, connection_id, &today, &overview)
        .await?;

    let inserted = sqlx::query(
        "INSERT INTO portfolio_snapshot (snapshot_date, portfolio_value, portfolio_yield, portfolio_count,
         ytd_interest, ytd_principal, trust_balance, outstanding_checks, service_fees)
         VALUES ($1::date, $2, $3, $4, $5, $6, $7, $8, $9)
         ON CONFLICT(snapshot_date) DO UPDATE SET
           portfolio_value = excluded.portfolio_value,
           portfolio_yield = excluded.portfolio_yield,
           portfolio_count = excluded.portfolio_count,
           ytd_interest = excluded.ytd_interest,
           ytd_principal = excluded.ytd_principal,
           trust_balance = excluded.trust_balance,
           outstanding_checks = excluded.outstanding_checks,
           service_fees = excluded.service_fees,
           synced_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
    )
    .bind(&today)
    .bind(overview.portfolio_value)
    .bind(overview.portfolio_yield)
    .bind(overview.portfolio_count)
    .bind(overview.ytd_interest)
    .bind(overview.ytd_principal)
    .bind(overview.trust_balance)
    .bind(overview.outstanding_checks_value)
    .bind(overview.ytd_serv_fees)
    .execute(pool)
    .await?;
    if inserted.rows_affected() > 0 {
        snapshots_created += 1;
    }

    // 2. Portfolio → tmo_loan + stream_schedule
    tracing::info!("syncing portfolio...");
    let loans = client.get_portfolio().await?;
    endpoints.push("portfolio");
    for loan in &loans {
        crate::db::integrations::upsert_tmo_import_loan_summary(
            pool,
            connection_id,
            stream_id,
            loan,
        )
        .await?;
        loans_upserted += 1;
    }

    // 3. Loan details (enriches tmo_loan with property info)
    tracing::info!("syncing loan details...");
    for loan in &loans {
        match client.get_loan_detail(&loan.loan_account).await {
            Ok(detail) => {
                crate::db::integrations::upsert_tmo_import_loan_detail(
                    pool,
                    connection_id,
                    &detail,
                )
                .await?;
                if let Err(error) =
                    crate::property_media::enrich_loan_workspace(pool, connection_id, &detail).await
                {
                    tracing::warn!(
                        "property media enrichment failed for loan {}: {}",
                        detail.loan_account,
                        error
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    "failed to fetch detail for loan {}: {}",
                    loan.loan_account,
                    e
                );
            }
        }
    }
    endpoints.push("loanDetail");

    // 4. Payment history → stream_event
    tracing::info!("syncing payment history...");
    let payments = client.get_history(None).await?;
    endpoints.push("history");
    crate::db::integrations::replace_tmo_import_payments(pool, connection_id, &payments).await?;
    let mut history_tx = pool.begin().await?;

    sqlx::query("DELETE FROM stream_event WHERE stream_id = $1 AND source_type = 'tmo_history'")
        .bind(stream_id)
        .execute(&mut *history_tx)
        .await?;

    for payment in crate::db::integrations::list_tmo_import_payments(pool, connection_id).await? {
        let label = format!("{} - {}", payment.borrower_name, payment.property_name);
        let check_date = payment.check_date.as_str();
        let normalized_check_number = payment.check_number.trim();
        let is_pending_print_check = normalized_check_number.is_empty()
            || normalized_check_number.eq_ignore_ascii_case("print");
        let amount_cents = (payment.amount * 100.0).round() as i64;
        let source_id = format!(
            "history:{}:{}:{}",
            payment.loan_account, check_date, amount_cents
        );
        let status = "received";
        let expected_date: Option<&str> = None;
        let actual_date = Some(check_date);

        let metadata = serde_json::json!({
            "check_number": payment.check_number,
            "is_pending_print_check": is_pending_print_check,
            "interest": payment.interest,
            "principal": payment.principal,
            "service_fee": payment.service_fee,
            "charges": payment.charges,
            "late_charges": payment.late_charges,
            "other": payment.other,
            "loan_account": payment.loan_account,
            "tmo_check_date": check_date,
        });

        let result = sqlx::query(
            "INSERT INTO stream_event (stream_id, label, scheduled_date, expected_date, actual_date, amount, status,
             source_id, source_type, metadata)
             VALUES ($1, $2, $3::date, $4::date, $5::date, $6, $7, $8, 'tmo_history', $9)
             ON CONFLICT(stream_id, source_type, source_id) DO UPDATE SET
               label = excluded.label,
               scheduled_date = excluded.scheduled_date,
               expected_date = excluded.expected_date,
               actual_date = excluded.actual_date,
               amount = excluded.amount,
               status = excluded.status,
               metadata = excluded.metadata,
               updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
        )
        .bind(stream_id)
        .bind(&label)
        .bind(check_date)
        .bind(expected_date)
        .bind(actual_date)
        .bind(payment.amount)
        .bind(status)
        .bind(&source_id)
        .bind(metadata.to_string())
        .execute(&mut *history_tx)
        .await?;

        if result.rows_affected() > 0 {
            events_upserted += 1;
        }

        crate::db::integrations::mark_payment_normalized(pool, payment.id, &source_id).await?;
    }

    history_tx.commit().await?;

    tracing::info!(
        "sync complete: {} loans, {} events, {} snapshots",
        loans_upserted,
        events_upserted,
        snapshots_created
    );
    crate::db::integrations::mark_connection_synced(pool, TMO_CONNECTION_SLUG).await?;

    Ok(SyncSummary {
        endpoints_hit: endpoints.join(","),
        loans_upserted,
        events_upserted,
        snapshots_created,
    })
}
