use serde::Serialize;
use sqlx::SqlitePool;

/// A single row in the forecast projection.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ForecastRow {
    pub event_id: i64,
    pub date: String,
    pub label: Option<String>,
    pub stream_name: Option<String>,
    pub amount: f64,
    pub status: String,
    pub source_type: Option<String>,
    pub metadata: Option<String>,
    pub is_delinquent: Option<i32>,
}

/// Full forecast response.
#[derive(Debug, Serialize)]
pub struct ForecastResponse {
    pub starting_balance: f64,
    pub rows: Vec<ForecastRowWithBalance>,
    pub ending_balance: f64,
}

/// A forecast row with computed running balance.
#[derive(Debug, Serialize)]
pub struct ForecastRowWithBalance {
    pub event_id: i64,
    pub date: String,
    pub label: Option<String>,
    pub stream_name: Option<String>,
    pub amount: f64,
    pub running_balance: f64,
    pub status: String,
    pub source_type: Option<String>,
    pub metadata: Option<String>,
    pub is_delinquent: Option<i32>,
}

/// Get the starting balance: manual setting first, then portfolio snapshot fallback.
pub async fn get_starting_balance(pool: &SqlitePool) -> Option<f64> {
    // Try manual setting first
    let manual: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM settings WHERE key = 'current_cash'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    if let Some((value,)) = manual {
        if let Ok(v) = value.parse::<f64>() {
            return Some(v);
        }
    }

    // Fallback to latest portfolio snapshot trust_balance
    let snapshot: Option<(Option<f64>,)> = sqlx::query_as(
        "SELECT trust_balance FROM portfolio_snapshot ORDER BY snapshot_date DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    snapshot.and_then(|(v,)| v)
}

/// Set the current cash balance.
pub async fn set_starting_balance(pool: &SqlitePool, amount: f64) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('current_cash', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(amount.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// Get forecast events between two dates, ordered by effective date.
/// Joins with stream and tmo_loan for display names and delinquency status.
pub async fn get_forecast_events(
    pool: &SqlitePool,
    from: &str,
    through: &str,
    stream_id: Option<i64>,
) -> anyhow::Result<Vec<ForecastRow>> {
    let stream_filter = match stream_id {
        Some(_) => "AND e.stream_id = ?",
        None => "",
    };

    let sql = format!(
        "SELECT e.id as event_id,
                COALESCE(e.expected_date, e.scheduled_date) as date,
                e.label, s.name as stream_name,
                e.amount, e.status, e.source_type, e.metadata,
                tl.is_delinquent
         FROM stream_event e
         JOIN stream s ON e.stream_id = s.id
         LEFT JOIN tmo_loan tl ON json_extract(e.metadata, '$.loan_account') = tl.loan_account
         WHERE COALESCE(e.expected_date, e.scheduled_date) BETWEEN ? AND ?
         {stream_filter}
         ORDER BY COALESCE(e.expected_date, e.scheduled_date) ASC, e.id ASC"
    );

    let mut query = sqlx::query_as::<_, ForecastRow>(&sql)
        .bind(from)
        .bind(through);

    if let Some(sid) = stream_id {
        query = query.bind(sid);
    }

    Ok(query.fetch_all(pool).await?)
}

/// Compute the full forecast with running balances.
pub async fn compute_forecast(
    pool: &SqlitePool,
    from: &str,
    through: &str,
    stream_id: Option<i64>,
) -> anyhow::Result<Option<ForecastResponse>> {
    let starting_balance = match get_starting_balance(pool).await {
        Some(b) => b,
        None => return Ok(None),
    };

    let events = get_forecast_events(pool, from, through, stream_id).await?;

    let mut running = starting_balance;
    let rows: Vec<ForecastRowWithBalance> = events
        .into_iter()
        .map(|e| {
            running += e.amount;
            ForecastRowWithBalance {
                event_id: e.event_id,
                date: e.date,
                label: e.label,
                stream_name: e.stream_name,
                amount: e.amount,
                running_balance: running,
                status: e.status,
                source_type: e.source_type,
                metadata: e.metadata,
                is_delinquent: e.is_delinquent,
            }
        })
        .collect();

    let ending_balance = running;

    Ok(Some(ForecastResponse {
        starting_balance,
        rows,
        ending_balance,
    }))
}

/// Estimate the average service fee per payment from historical data.
/// Returns the average service_fee from received payments, or 0.0 if no history.
pub async fn estimate_service_fee(pool: &SqlitePool) -> f64 {
    let result: Option<(f64,)> = sqlx::query_as(
        "SELECT AVG(CAST(json_extract(metadata, '$.service_fee') AS REAL))
         FROM stream_event
         WHERE source_type = 'tmo_history'
         AND json_extract(metadata, '$.service_fee') IS NOT NULL",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    // TMO stores service fees as negative values; return absolute value
    result.map(|(v,)| v.abs()).unwrap_or(0.0)
}
