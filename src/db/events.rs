use sqlx::PgPool;

use crate::models::{LoanPaymentHistoryView, PaymentView};

/// Get recent received payments, most recent first.
pub async fn get_recent_payments(pool: &PgPool, limit: i32) -> Vec<PaymentView> {
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
         WHERE status = 'received'
         ORDER BY actual_date DESC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

/// Get all payments ordered by effective date, most recent first.
pub async fn get_all_payments(pool: &PgPool, limit: i32) -> Vec<PaymentView> {
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
         ORDER BY COALESCE(actual_date, scheduled_date) DESC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

/// Get pulled payment history for the payments pages.
pub async fn get_payments(pool: &PgPool, limit: i32) -> Vec<PaymentView> {
    get_recent_payments(pool, limit).await
}

pub async fn get_payments_for_loan(
    pool: &PgPool,
    loan_account: &str,
    limit: i32,
) -> Vec<LoanPaymentHistoryView> {
    let rows: Vec<PaymentView> = sqlx::query_as(
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
         WHERE NULLIF(NULLIF(metadata, '')::jsonb ->> 'loan_account', '') = $1
         ORDER BY COALESCE(actual_date, expected_date, scheduled_date) DESC, id DESC
         LIMIT $2",
    )
    .bind(loan_account)
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    rows.into_iter().map(map_payment_history).collect()
}

/// Create a new stream event (for manual outflows or overrides).
pub async fn create_event(
    pool: &PgPool,
    stream_id: i64,
    account_id: Option<i64>,
    label: &str,
    scheduled_date: &str,
    amount: f64,
    status: &str,
    source_type: &str,
    metadata: Option<&str>,
) -> anyhow::Result<i64> {
    let source_id = format!("manual:{}", chrono::Utc::now().timestamp_millis());
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO stream_event (stream_id, account_id, label, scheduled_date, amount, status,
         source_id, source_type, metadata)
         VALUES ($1, $2, $3, $4::date, $5, $6, $7, $8, $9) RETURNING id",
    )
    .bind(stream_id)
    .bind(account_id)
    .bind(label)
    .bind(scheduled_date)
    .bind(amount)
    .bind(status)
    .bind(&source_id)
    .bind(source_type)
    .bind(metadata)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// Update editable event fields on a projected/confirmed event.
pub async fn update_event(
    pool: &PgPool,
    event_id: i64,
    label: Option<&str>,
    amount: Option<f64>,
    expected_date: Option<&str>,
    account_id: Option<i64>,
) -> anyhow::Result<bool> {
    let result = sqlx::query(
        "UPDATE stream_event
         SET label = COALESCE($1, label),
             amount = COALESCE($2, amount),
             expected_date = COALESCE($3::date, expected_date),
             account_id = COALESCE($4, account_id),
             metadata = CASE
                WHEN $3 IS NULL THEN metadata
                ELSE jsonb_set(
                    COALESCE(NULLIF(metadata, '')::jsonb, '{}'::jsonb),
                    '{user_override}',
                    'true'::jsonb
                )::text
             END,
             updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
         WHERE id = $5 AND status NOT IN ('received')",
    )
    .bind(label)
    .bind(amount)
    .bind(expected_date)
    .bind(account_id)
    .bind(event_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Backwards-compatible helper for overriding just the expected date.
pub async fn override_event_date(
    pool: &PgPool,
    event_id: i64,
    expected_date: &str,
) -> anyhow::Result<bool> {
    update_event(pool, event_id, None, None, Some(expected_date), None).await
}

/// Delete stale projected events (scheduled_date in the past, never received).
pub async fn cleanup_stale_projections(pool: &PgPool) -> anyhow::Result<u64> {
    let result = sqlx::query(
        "DELETE FROM stream_event
         WHERE status = 'projected'
         AND scheduled_date < CURRENT_DATE
         AND actual_date IS NULL",
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Delete projected TMO schedule events so sync no longer leaves payment projections behind.
pub async fn cleanup_tmo_projections(pool: &PgPool) -> anyhow::Result<u64> {
    let result = sqlx::query(
        "DELETE FROM stream_event
         WHERE status = 'projected'
           AND source_type = 'schedule'",
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

fn map_payment_history(payment: PaymentView) -> LoanPaymentHistoryView {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let effective_date = payment
        .actual_date
        .clone()
        .or_else(|| payment.expected_date.clone())
        .unwrap_or_else(|| payment.scheduled_date.clone());
    let display_date = chrono::NaiveDate::parse_from_str(&effective_date, "%Y-%m-%d")
        .map(|date| date.format("%m/%d/%Y").to_string())
        .unwrap_or_else(|_| effective_date.clone());

    let timing_label = if let Some(actual_date) = payment.actual_date.clone() {
        if actual_date > payment.scheduled_date {
            "Paid late".to_string()
        } else {
            "Paid on time".to_string()
        }
    } else if let Some(expected_date) = payment.expected_date.clone() {
        if expected_date > payment.scheduled_date {
            "Expected late".to_string()
        } else {
            "Pending on time".to_string()
        }
    } else if payment.scheduled_date < today {
        "Overdue".to_string()
    } else {
        "Scheduled".to_string()
    };

    let state_label = if payment.actual_date.is_some() {
        "Actual".to_string()
    } else if payment.expected_date.is_some() {
        "Expected".to_string()
    } else {
        "Scheduled".to_string()
    };

    LoanPaymentHistoryView {
        id: payment.id,
        label: payment.label,
        effective_date,
        display_date,
        scheduled_date: payment.scheduled_date,
        expected_date: payment.expected_date,
        actual_date: payment.actual_date,
        amount: payment.amount,
        status: payment.status,
        state_label,
        timing_label,
        check_number: payment.check_number,
        source_type: payment.source_type,
    }
}
