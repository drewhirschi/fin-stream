use sqlx::SqlitePool;

use crate::models::*;

/// Run a full sync: login, fetch overview, portfolio, loan details, payment history.
/// Returns a SyncLog-like summary.
pub async fn run_full_sync(pool: &SqlitePool) -> anyhow::Result<SyncSummary> {
    let now = chrono::Utc::now().to_rfc3339();

    // Insert sync_log row as "running"
    let (log_id,): (i64,) = sqlx::query_as(
        "INSERT INTO sync_log (started_at, status) VALUES (?, 'running') RETURNING id",
    )
    .bind(&now)
    .fetch_one(pool)
    .await?;

    let result = run_sync_inner(pool).await;

    let finished = chrono::Utc::now().to_rfc3339();

    match &result {
        Ok(summary) => {
            sqlx::query(
                "UPDATE sync_log SET finished_at = ?, status = 'success',
                 endpoints_hit = ?, events_upserted = ?, loans_upserted = ?, snapshots_created = ?
                 WHERE id = ?",
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
            sqlx::query(
                "UPDATE sync_log SET finished_at = ?, status = 'error', error_message = ? WHERE id = ?",
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

async fn run_sync_inner(pool: &SqlitePool) -> anyhow::Result<SyncSummary> {
    let client = crate::tmo::client::create_client().await?;
    let stream_id = crate::db::ensure_trustee_stream(pool).await?;
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let mut endpoints = Vec::new();
    let mut loans_upserted = 0;
    let mut events_upserted = 0;
    let mut snapshots_created = 0;

    // Clean up stale projected events before generating new ones
    let stale_count = crate::db::events::cleanup_stale_projections(pool).await?;
    if stale_count > 0 {
        tracing::info!("cleaned up {} stale projected events", stale_count);
    }

    // 1. Overview → portfolio_snapshot
    tracing::info!("syncing overview...");
    let overview = client.get_overview().await?;
    endpoints.push("overview");

    let inserted = sqlx::query(
        "INSERT INTO portfolio_snapshot (snapshot_date, portfolio_value, portfolio_yield, portfolio_count,
         ytd_interest, ytd_principal, trust_balance, outstanding_checks, service_fees)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(snapshot_date) DO UPDATE SET
           portfolio_value = excluded.portfolio_value,
           portfolio_yield = excluded.portfolio_yield,
           portfolio_count = excluded.portfolio_count,
           ytd_interest = excluded.ytd_interest,
           ytd_principal = excluded.ytd_principal,
           trust_balance = excluded.trust_balance,
           outstanding_checks = excluded.outstanding_checks,
           service_fees = excluded.service_fees,
           synced_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
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
        sqlx::query(
            "INSERT INTO tmo_loan (loan_account, stream_id, borrower_name, property_address, property_city,
             property_state, property_zip, percent_owned, note_rate, principal_balance,
             regular_payment, maturity_date, next_payment_date, interest_paid_to,
             term_left_months, is_delinquent, is_active, last_synced_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(loan_account) DO UPDATE SET
               borrower_name = excluded.borrower_name,
               property_address = excluded.property_address,
               property_city = excluded.property_city,
               property_state = excluded.property_state,
               property_zip = excluded.property_zip,
               percent_owned = excluded.percent_owned,
               note_rate = excluded.note_rate,
               principal_balance = excluded.principal_balance,
               regular_payment = excluded.regular_payment,
               maturity_date = excluded.maturity_date,
               next_payment_date = excluded.next_payment_date,
               interest_paid_to = excluded.interest_paid_to,
               term_left_months = excluded.term_left_months,
               is_delinquent = excluded.is_delinquent,
               is_active = 1,
               last_synced_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
               updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )
        .bind(&loan.loan_account)
        .bind(stream_id)
        .bind(&loan.borrower_name)
        .bind(&loan.primary_street)
        .bind(&loan.primary_city)
        .bind(&loan.primary_state)
        .bind(&loan.primary_zip)
        .bind(loan.percent_owned)
        .bind(loan.interest_rate)
        .bind(loan.loan_balance)
        .bind(loan.regular_payment)
        .bind(&loan.maturity_date)
        .bind(&loan.next_payment_date)
        .bind(&loan.interest_paid_to_date)
        .bind(loan.term_left)
        .bind(loan.is_delinquent)
        .execute(pool)
        .await?;
        loans_upserted += 1;
    }

    // 3. Loan details (enriches tmo_loan with property info)
    tracing::info!("syncing loan details...");
    for loan in &loans {
        match client.get_loan_detail(&loan.loan_account).await {
            Ok(detail) => {
                sqlx::query(
                    "UPDATE tmo_loan SET
                       property_type = ?, property_priority = ?, occupancy = ?,
                       appraised_value = ?, ltv = ?, original_balance = ?,
                       loan_type = ?, payment_frequency = ?,
                       detail_synced_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE loan_account = ?",
                )
                .bind(&detail.property_type)
                .bind(detail.property_priority)
                .bind(&detail.occupancy)
                .bind(detail.appraised_value)
                .bind(detail.ltv)
                .bind(detail.original_balance)
                .bind(detail.loan_type)
                .bind(&detail.payment_frequency)
                .bind(&detail.loan_account)
                .execute(pool)
                .await?;
            }
            Err(e) => {
                tracing::warn!("failed to fetch detail for loan {}: {}", loan.loan_account, e);
            }
        }
    }
    endpoints.push("loanDetail");

    // 4. Payment history → stream_event
    tracing::info!("syncing payment history...");
    let payments = client.get_history(None).await?;
    endpoints.push("history");

    for payment in &payments {
        let label = format!(
            "{} - {}",
            payment.borrower_name, payment.property_name
        );

        let metadata = serde_json::json!({
            "check_number": payment.check_number,
            "interest": payment.interest,
            "principal": payment.principal,
            "service_fee": payment.service_fee,
            "charges": payment.charges,
            "late_charges": payment.late_charges,
            "other": payment.other,
            "loan_account": payment.loan_account,
        });

        // Extract just the date part from the datetime string
        let check_date = payment.check_date.split('T').next().unwrap_or(&payment.check_date);

        let result = sqlx::query(
            "INSERT INTO stream_event (stream_id, label, scheduled_date, actual_date, amount, status,
             source_id, source_type, metadata)
             VALUES (?, ?, ?, ?, ?, 'received', ?, 'tmo_history', ?)
             ON CONFLICT(stream_id, source_type, source_id) DO UPDATE SET
               label = excluded.label,
               actual_date = excluded.actual_date,
               amount = excluded.amount,
               metadata = excluded.metadata,
               updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )
        .bind(stream_id)
        .bind(&label)
        .bind(check_date)
        .bind(check_date)
        .bind(payment.amount)
        .bind(&payment.check_number)
        .bind(metadata.to_string())
        .execute(pool)
        .await?;

        if result.rows_affected() > 0 {
            events_upserted += 1;
        }
    }

    // 5. Estimate service fee from historical payments for projection accuracy
    let avg_service_fee = crate::db::forecasts::estimate_service_fee(pool).await;
    tracing::info!("estimated avg service fee: ${:.2}", avg_service_fee);

    // 6. Generate projected future events from loan schedules
    tracing::info!("generating projected events...");
    let projected = generate_projected_events(pool, stream_id, &loans, avg_service_fee).await?;
    events_upserted += projected;

    tracing::info!(
        "sync complete: {} loans, {} events, {} snapshots",
        loans_upserted, events_upserted, snapshots_created
    );

    Ok(SyncSummary {
        endpoints_hit: endpoints.join(","),
        loans_upserted,
        events_upserted,
        snapshots_created,
    })
}

/// Generate projected future payment events for the next 6 months based on loan data.
/// Subtracts the estimated average service fee from each projected payment for accuracy.
async fn generate_projected_events(
    pool: &SqlitePool,
    stream_id: i64,
    loans: &[TmoLoanSummary],
    avg_service_fee: f64,
) -> anyhow::Result<i32> {
    let mut count = 0;
    let today = chrono::Utc::now().date_naive();

    for loan in loans {
        // Parse the next payment date
        let next_date_str = loan.next_payment_date.split('T').next().unwrap_or(&loan.next_payment_date);
        let Ok(mut date) = chrono::NaiveDate::parse_from_str(next_date_str, "%Y-%m-%d") else {
            tracing::warn!("could not parse next_payment_date for loan {}: {}", loan.loan_account, loan.next_payment_date);
            continue;
        };

        // Parse maturity date
        let maturity_str = loan.maturity_date.split('T').next().unwrap_or(&loan.maturity_date);
        let maturity = chrono::NaiveDate::parse_from_str(maturity_str, "%Y-%m-%d").unwrap_or(
            today + chrono::Duration::days(365),
        );

        let label = format!(
            "{} - {}, {} {}",
            loan.borrower_name, loan.primary_street, loan.primary_city, loan.primary_state
        );

        // Generate monthly events for up to 6 months into the future
        let horizon = today + chrono::Duration::days(180);

        while date <= horizon && date <= maturity {
            if date >= today {
                let source_id = format!("projected:{}:{}", loan.loan_account, date);

                let result = sqlx::query(
                    "INSERT INTO stream_event (stream_id, label, scheduled_date, amount, status,
                     source_id, source_type, metadata)
                     VALUES (?, ?, ?, ?, 'projected', ?, 'schedule', ?)
                     ON CONFLICT(stream_id, source_type, source_id) DO UPDATE SET
                       label = excluded.label,
                       amount = excluded.amount,
                       metadata = excluded.metadata,
                       updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                )
                .bind(stream_id)
                .bind(&label)
                .bind(date.to_string())
                .bind(loan.regular_payment - avg_service_fee)
                .bind(&source_id)
                .bind(serde_json::json!({ "loan_account": loan.loan_account }).to_string())
                .execute(pool)
                .await?;

                if result.rows_affected() > 0 {
                    count += 1;
                }
            }

            // Advance one month using chrono
            date = date
                .checked_add_months(chrono::Months::new(1))
                .unwrap_or(date + chrono::Duration::days(30));
        }
    }

    Ok(count)
}
