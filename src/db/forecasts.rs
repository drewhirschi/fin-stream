use serde::Serialize;
use sqlx::PgPool;

use crate::db::accounts;
use crate::models::CashSourceView;

/// A single row in the forecast projection.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ForecastRow {
    pub event_id: i64,
    pub stream_id: i64,
    pub account_id: Option<i64>,
    pub date: String,
    pub expected_date: String,
    pub actual_date: Option<String>,
    pub label: Option<String>,
    pub stream_name: Option<String>,
    pub account_name: Option<String>,
    pub amount: f64,
    pub status: String,
    pub source_type: Option<String>,
    pub metadata: Option<String>,
}

/// Full forecast response.
#[derive(Debug, Serialize)]
pub struct ForecastResponse {
    pub starting_balance: f64,
    pub cash_source: Option<CashSourceView>,
    pub rows: Vec<ForecastRowWithBalance>,
    pub ending_balance: f64,
}

/// A forecast row with computed running balances.
#[derive(Debug, Serialize)]
pub struct ForecastRowWithBalance {
    pub event_id: i64,
    pub stream_id: i64,
    pub account_id: Option<i64>,
    pub date: String,
    pub expected_date: String,
    pub actual_date: Option<String>,
    pub label: Option<String>,
    pub stream_name: Option<String>,
    pub account_name: Option<String>,
    pub amount: f64,
    pub running_balance: f64,
    pub status: String,
    pub source_type: Option<String>,
    pub metadata: Option<String>,
    /// Derived from dates — true iff the event was expected in the past and
    /// hasn't been received yet. Replaces the TMO-specific `is_delinquent`.
    pub is_late: bool,
}

/// Get the starting balance: primary account first, then manual setting, then portfolio snapshot.
pub async fn get_starting_balance(pool: &PgPool) -> Option<f64> {
    let primary_balance: Option<(Option<f64>,)> = sqlx::query_as(
        "SELECT balance
         FROM account
         WHERE is_primary = 1 AND is_active = 1
         ORDER BY id ASC
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    if let Some((Some(balance),)) = primary_balance {
        return Some(balance);
    }

    let manual: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'current_cash'")
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();

    if let Some((value,)) = manual {
        if let Ok(v) = value.parse::<f64>() {
            return Some(v);
        }
    }

    let snapshot: Option<(Option<f64>,)> = sqlx::query_as(
        "SELECT trust_balance
         FROM portfolio_snapshot
         ORDER BY snapshot_date DESC
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    snapshot.and_then(|(value,)| value)
}

/// Set the current cash balance.
pub async fn set_starting_balance(pool: &PgPool, amount: f64) -> anyhow::Result<()> {
    accounts::set_primary_balance(pool, amount, "manual", None, None, None).await
}

/// Get forecast events between two dates, ordered by effective date.
pub async fn get_forecast_events(
    pool: &PgPool,
    from: &str,
    through: &str,
    stream_id: Option<i64>,
    view_id: Option<i64>,
) -> anyhow::Result<Vec<ForecastRow>> {
    let rows = sqlx::query_as(
        "SELECT e.id as event_id,
                e.stream_id,
                e.account_id,
                e.expected_date::text as date,
                e.expected_date::text as expected_date,
                e.actual_date::text as actual_date,
                e.label,
                s.name as stream_name,
                a.name as account_name,
                e.amount,
                e.status,
                e.source_type,
                e.metadata
         FROM stream_event e
         JOIN stream s ON e.stream_id = s.id
         LEFT JOIN account a ON a.id = COALESCE(e.account_id, s.default_account_id)
         WHERE e.expected_date BETWEEN $1::date AND $2::date
           AND ($3::bigint IS NULL OR e.stream_id = $3)
           AND (
                $4::bigint IS NULL
                OR EXISTS (
                    SELECT 1
                    FROM stream_view_stream svs
                    WHERE svs.stream_view_id = $4
                      AND svs.stream_id = e.stream_id
                )
           )
         ORDER BY e.expected_date ASC, e.id ASC",
    )
    .bind(from)
    .bind(through)
    .bind(stream_id)
    .bind(view_id)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// Compute the full forecast with running balances.
pub async fn compute_forecast(
    pool: &PgPool,
    from: &str,
    through: &str,
    stream_id: Option<i64>,
    view_id: Option<i64>,
) -> anyhow::Result<Option<ForecastResponse>> {
    let starting_balance = match get_starting_balance(pool).await {
        Some(balance) => balance,
        None => return Ok(None),
    };
    let cash_source = accounts::get_cash_source(pool).await;
    let events = get_forecast_events(pool, from, through, stream_id, view_id).await?;

    let today = chrono::Utc::now().date_naive();

    let mut running = starting_balance;
    let rows = events
        .into_iter()
        .map(|event| {
            running += event.amount;
            let is_late = event.actual_date.is_none()
                && chrono::NaiveDate::parse_from_str(&event.expected_date, "%Y-%m-%d")
                    .map(|d| d < today)
                    .unwrap_or(false)
                && matches!(event.status.as_str(), "projected" | "confirmed");
            ForecastRowWithBalance {
                event_id: event.event_id,
                stream_id: event.stream_id,
                account_id: event.account_id,
                date: event.date,
                expected_date: event.expected_date,
                actual_date: event.actual_date,
                label: event.label,
                stream_name: event.stream_name,
                account_name: event.account_name,
                amount: event.amount,
                running_balance: running,
                status: event.status,
                source_type: event.source_type,
                metadata: event.metadata,
                is_late,
            }
        })
        .collect();

    Ok(Some(ForecastResponse {
        starting_balance,
        cash_source,
        rows,
        ending_balance: running,
    }))
}
