use sqlx::SqlitePool;

use crate::models::PaymentView;

/// Get recent received payments, most recent first.
pub async fn get_recent_payments(pool: &SqlitePool, limit: i32) -> Vec<PaymentView> {
    sqlx::query_as(
        "SELECT id, label, scheduled_date, actual_date, amount, status, metadata
         FROM stream_event
         WHERE status = 'received'
         ORDER BY actual_date DESC
         LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

/// Get upcoming projected payments, soonest first.
pub async fn get_upcoming_payments(pool: &SqlitePool, limit: i32) -> Vec<PaymentView> {
    sqlx::query_as(
        "SELECT id, label, scheduled_date, actual_date, amount, status, metadata
         FROM stream_event
         WHERE status = 'projected'
         ORDER BY scheduled_date ASC
         LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

/// Get all payments ordered by effective date, most recent first.
pub async fn get_all_payments(pool: &SqlitePool, limit: i32) -> Vec<PaymentView> {
    sqlx::query_as(
        "SELECT id, label, scheduled_date, actual_date, amount, status, metadata
         FROM stream_event
         ORDER BY COALESCE(actual_date, scheduled_date) DESC
         LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

/// Create a new stream event (for manual outflows or overrides).
pub async fn create_event(
    pool: &SqlitePool,
    stream_id: i64,
    label: &str,
    scheduled_date: &str,
    amount: f64,
    status: &str,
    source_type: &str,
    metadata: Option<&str>,
) -> anyhow::Result<i64> {
    let source_id = format!("manual:{}", chrono::Utc::now().timestamp_millis());
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO stream_event (stream_id, label, scheduled_date, amount, status,
         source_id, source_type, metadata)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(stream_id)
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

/// Update the expected_date on a projected event (override).
pub async fn override_event_date(
    pool: &SqlitePool,
    event_id: i64,
    expected_date: &str,
) -> anyhow::Result<bool> {
    // Only allow overriding projected/confirmed events, not received ones
    let result = sqlx::query(
        "UPDATE stream_event
         SET expected_date = ?,
             metadata = json_set(COALESCE(metadata, '{}'), '$.user_override', json('true')),
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND status NOT IN ('received')",
    )
    .bind(expected_date)
    .bind(event_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Delete stale projected events (scheduled_date in the past, never received).
pub async fn cleanup_stale_projections(pool: &SqlitePool) -> anyhow::Result<u64> {
    let result = sqlx::query(
        "DELETE FROM stream_event
         WHERE status = 'projected'
         AND scheduled_date < date('now')
         AND actual_date IS NULL",
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}
