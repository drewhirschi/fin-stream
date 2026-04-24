use sqlx::PgPool;

use crate::models::PaymentView;

/// Get recent received payments, most recent first.
pub async fn get_recent_payments(pool: &PgPool, limit: i32) -> Vec<PaymentView> {
    sqlx::query_as(
        "SELECT id,
                label,
                expected_date::text as expected_date,
                actual_date::text as actual_date,
                amount,
                status,
                source_type,
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

/// Create a new stream event (for manual outflows or overrides).
pub async fn create_event(
    pool: &PgPool,
    stream_id: i64,
    account_id: Option<i64>,
    label: &str,
    expected_date: &str,
    amount: f64,
    status: &str,
    source_type: &str,
    metadata: Option<&str>,
) -> anyhow::Result<i64> {
    let source_id = format!("manual:{}", chrono::Utc::now().timestamp_millis());
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO stream_event (stream_id, account_id, label, expected_date, amount, status,
         source_id, source_type, metadata)
         VALUES ($1, $2, $3, $4::date, $5, $6, $7, $8, $9) RETURNING id",
    )
    .bind(stream_id)
    .bind(account_id)
    .bind(label)
    .bind(expected_date)
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

/// Delete stale projected events (expected_date in the past, never received).
pub async fn cleanup_stale_projections(pool: &PgPool) -> anyhow::Result<u64> {
    let result = sqlx::query(
        "DELETE FROM stream_event
         WHERE status = 'projected'
         AND expected_date < CURRENT_DATE
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
