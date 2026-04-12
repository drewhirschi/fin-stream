use anyhow::Context;
use chrono::{Datelike, Months, NaiveDate, Utc};
use sqlx::PgPool;

use crate::db::accounts;
use crate::models::{StreamConfigView, StreamViewEditor, StreamViewMember, StreamViewSummary};

const DEFAULT_HORIZON_DAYS: i64 = 365;

pub async fn ensure_default_configuration(pool: &PgPool) -> anyhow::Result<()> {
    let primary_account_id = accounts::ensure_primary_account(pool).await?;
    let trust_stream_id = ensure_stream(
        pool,
        "Trust Deeds",
        "mortgage_portfolio",
        "tmo_trust",
        Some("Payments flowing from The Mortgage Office into your trust deed cash view."),
        Some(primary_account_id),
        &["mortgage_portfolio", "Trustee Income"],
    )
    .await?;

    let _ = ensure_stream(
        pool,
        "One-off Income",
        "manual_income",
        "manual_income",
        Some("Manual one-time inflows."),
        Some(primary_account_id),
        &["manual_income", "Income"],
    )
    .await?;

    let _ = ensure_stream(
        pool,
        "One-off Expense",
        "manual_expense",
        "manual_expense",
        Some("Manual one-time outflows and bills."),
        Some(primary_account_id),
        &["manual_expense", "expenses", "Expenses"],
    )
    .await?;

    let hannah_costco = ensure_stream(
        pool,
        "Hannah Costco",
        "credit_card_due",
        "credit_card",
        Some("Monthly Hannah Costco payment due date."),
        Some(primary_account_id),
        &["Hannah Costco"],
    )
    .await?;
    let drew_costco = ensure_stream(
        pool,
        "Drew Costco",
        "credit_card_due",
        "credit_card",
        Some("Monthly Drew Costco payment due date."),
        Some(primary_account_id),
        &["Drew Costco"],
    )
    .await?;
    let apple_card = ensure_stream(
        pool,
        "Apple Card",
        "credit_card_due",
        "credit_card",
        Some("Monthly Apple Card payment due date."),
        Some(primary_account_id),
        &["Apple Card"],
    )
    .await?;

    let seed_start = Utc::now()
        .date_naive()
        .with_day(1)
        .unwrap_or_else(|| Utc::now().date_naive());

    ensure_monthly_schedule(
        pool,
        hannah_costco,
        "Hannah Costco due",
        0.0,
        21,
        &seed_start.to_string(),
        Some(primary_account_id),
    )
    .await?;
    ensure_monthly_schedule(
        pool,
        drew_costco,
        "Drew Costco due",
        0.0,
        22,
        &seed_start.to_string(),
        Some(primary_account_id),
    )
    .await?;
    ensure_monthly_schedule(
        pool,
        apple_card,
        "Apple Card due",
        0.0,
        31,
        &seed_start.to_string(),
        Some(primary_account_id),
    )
    .await?;

    sqlx::query("UPDATE intg.tmo_import_loan SET stream_id = $1 WHERE stream_id IS NULL")
        .bind(trust_stream_id)
        .execute(pool)
        .await?;

    let default_view_id = ensure_default_view(pool).await?;
    sync_default_view_membership(pool, default_view_id).await?;
    refresh_stream_schedule_events(pool).await?;

    Ok(())
}

async fn ensure_stream(
    pool: &PgPool,
    name: &str,
    stream_type: &str,
    kind: &str,
    description: Option<&str>,
    default_account_id: Option<i64>,
    aliases: &[&str],
) -> anyhow::Result<i64> {
    let mut candidates = vec![name.to_string(), stream_type.to_string()];
    candidates.extend(aliases.iter().map(|value| value.to_string()));

    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT id
         FROM stream
         WHERE name = ANY($1) OR type = ANY($1)
         ORDER BY id ASC
         LIMIT 1",
    )
    .bind(&candidates)
    .fetch_optional(pool)
    .await?;

    if let Some((id,)) = row {
        sqlx::query(
            "UPDATE stream
             SET name = $1,
                 type = $2,
                 kind = $3,
                 description = $4,
                 default_account_id = $5,
                 is_active = 1,
                 updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
             WHERE id = $6",
        )
        .bind(name)
        .bind(stream_type)
        .bind(kind)
        .bind(description)
        .bind(default_account_id)
        .bind(id)
        .execute(pool)
        .await?;
        return Ok(id);
    }

    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO stream (
            name, type, kind, description, default_account_id, is_active
         ) VALUES (
            $1, $2, $3, $4, $5, 1
         ) RETURNING id",
    )
    .bind(name)
    .bind(stream_type)
    .bind(kind)
    .bind(description)
    .bind(default_account_id)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

async fn ensure_default_view(pool: &PgPool) -> anyhow::Result<i64> {
    let existing: Option<(i64,)> = sqlx::query_as(
        "SELECT id
         FROM stream_view
         WHERE is_default = 1 OR name = 'All Streams'
         ORDER BY is_default DESC, id ASC
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;

    if let Some((id,)) = existing {
        sqlx::query(
            "UPDATE stream_view
             SET name = 'All Streams',
                 description = 'Merged view across every active stream.',
                 is_default = 1,
                 is_active = 1,
                 updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
             WHERE id = $1",
        )
        .bind(id)
        .execute(pool)
        .await?;
        return Ok(id);
    }

    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO stream_view (
            name, description, is_default, is_active
         ) VALUES (
            'All Streams', 'Merged view across every active stream.', 1, 1
         ) RETURNING id",
    )
    .fetch_one(pool)
    .await?;
    Ok(id)
}

async fn sync_default_view_membership(pool: &PgPool, view_id: i64) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO stream_view_stream (stream_view_id, stream_id)
         SELECT $1, s.id
         FROM stream s
         WHERE s.is_active = 1
           AND NOT EXISTS (
                SELECT 1
                FROM stream_view_stream svs
                WHERE svs.stream_view_id = $1
                  AND svs.stream_id = s.id
           )",
    )
    .bind(view_id)
    .execute(pool)
    .await?;

    Ok(())
}

async fn ensure_monthly_schedule(
    pool: &PgPool,
    stream_id: i64,
    label: &str,
    amount: f64,
    due_day: i32,
    start_date: &str,
    account_id: Option<i64>,
) -> anyhow::Result<i64> {
    let existing: Option<(i64,)> = sqlx::query_as(
        "SELECT id
         FROM stream_schedule
         WHERE stream_id = $1 AND frequency = 'monthly'
         ORDER BY id ASC
         LIMIT 1",
    )
    .bind(stream_id)
    .fetch_optional(pool)
    .await?;

    if let Some((id,)) = existing {
        sqlx::query(
            "UPDATE stream_schedule
             SET label = $1,
                 amount = $2,
                 day_of_month = $3,
                 start_date = $4::date,
                 account_id = $5,
                 is_active = 1,
                 updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
             WHERE id = $6",
        )
        .bind(label)
        .bind(amount)
        .bind(due_day)
        .bind(start_date)
        .bind(account_id)
        .bind(id)
        .execute(pool)
        .await?;
        return Ok(id);
    }

    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO stream_schedule (
            stream_id, label, amount, frequency, day_of_month, start_date, account_id, is_active
         ) VALUES (
            $1, $2, $3, 'monthly', $4, $5::date, $6, 1
         ) RETURNING id",
    )
    .bind(stream_id)
    .bind(label)
    .bind(amount)
    .bind(due_day)
    .bind(start_date)
    .bind(account_id)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

#[derive(sqlx::FromRow)]
struct ScheduleProjection {
    id: i64,
    stream_id: i64,
    label: Option<String>,
    amount: f64,
    frequency: String,
    day_of_month: Option<i32>,
    start_date: String,
    end_date: Option<String>,
    account_id: Option<i64>,
    stream_name: String,
}

pub async fn refresh_stream_schedule_events(pool: &PgPool) -> anyhow::Result<()> {
    let today = Utc::now().date_naive();

    sqlx::query(
        "DELETE FROM stream_event
         WHERE source_type = 'stream_schedule'
           AND status = 'projected'
           AND expected_date IS NULL
           AND scheduled_date >= $1::date",
    )
    .bind(today.to_string())
    .execute(pool)
    .await?;

    let schedules: Vec<ScheduleProjection> = sqlx::query_as(
        "SELECT ss.id, ss.stream_id, ss.label, ss.amount, ss.frequency,
                ss.day_of_month, ss.start_date::text as start_date, ss.end_date::text as end_date,
                COALESCE(ss.account_id, s.default_account_id) as account_id,
                s.name as stream_name
         FROM stream_schedule ss
         JOIN stream s ON s.id = ss.stream_id
         WHERE ss.is_active = 1
           AND s.is_active = 1",
    )
    .fetch_all(pool)
    .await?;

    let horizon = today + chrono::Duration::days(DEFAULT_HORIZON_DAYS);

    for schedule in schedules {
        if schedule.frequency != "monthly" {
            continue;
        }

        let start = NaiveDate::parse_from_str(&schedule.start_date, "%Y-%m-%d").unwrap_or(today);
        let effective_start = start.max(today);
        let end = schedule
            .end_date
            .as_deref()
            .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
            .unwrap_or(horizon)
            .min(horizon);

        for scheduled_date in
            monthly_occurrences(effective_start, end, schedule.day_of_month.unwrap_or(1))
        {
            let label = schedule
                .label
                .clone()
                .unwrap_or_else(|| format!("{} due", schedule.stream_name));
            let source_id = format!("stream_schedule:{}:{}", schedule.id, scheduled_date);
            let metadata = serde_json::json!({
                "schedule_id": schedule.id,
                "stream_name": schedule.stream_name,
            });

            sqlx::query(
                "INSERT INTO stream_event (
                    stream_id, account_id, label, scheduled_date, amount, status,
                    source_id, source_type, metadata
                 ) VALUES (
                    $1, $2, $3, $4::date, $5, 'projected', $6, 'stream_schedule', $7
                 )
                 ON CONFLICT(stream_id, source_type, source_id) DO UPDATE SET
                    account_id = excluded.account_id,
                    label = excluded.label,
                    scheduled_date = excluded.scheduled_date,
                    amount = excluded.amount,
                    metadata = excluded.metadata,
                    updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
            )
            .bind(schedule.stream_id)
            .bind(schedule.account_id)
            .bind(label)
            .bind(scheduled_date.to_string())
            .bind(schedule.amount)
            .bind(source_id)
            .bind(metadata.to_string())
            .execute(pool)
            .await?;
        }
    }

    Ok(())
}

fn monthly_occurrences(start: NaiveDate, end: NaiveDate, due_day: i32) -> Vec<NaiveDate> {
    let mut cursor = start.with_day(1).unwrap_or(start);
    let mut dates = Vec::new();

    while cursor <= end {
        let last_day = last_day_of_month(cursor);
        let day = due_day.max(1).min(last_day.day() as i32) as u32;
        let candidate = cursor.with_day(day).unwrap_or(cursor);
        if candidate >= start && candidate <= end {
            dates.push(candidate);
        }

        cursor = cursor
            .checked_add_months(Months::new(1))
            .unwrap_or(end + chrono::Duration::days(1));
    }

    dates
}

fn last_day_of_month(date: NaiveDate) -> NaiveDate {
    let first_of_next_month = date
        .with_day(1)
        .unwrap_or(date)
        .checked_add_months(Months::new(1))
        .unwrap_or(date);
    first_of_next_month - chrono::Duration::days(1)
}

pub async fn list_streams(pool: &PgPool) -> Vec<StreamConfigView> {
    sqlx::query_as(
        "SELECT s.id, s.name, s.type, COALESCE(s.kind, 'manual') as kind, s.description, s.is_active,
                COALESCE(s.default_account_id, 0) as default_account_id, a.name as default_account_name,
                ss.id as schedule_id, ss.label as schedule_label, ss.amount as schedule_amount,
                ss.frequency as schedule_frequency, ss.day_of_month as due_day,
                ss.start_date::text as schedule_start_date
         FROM stream s
         LEFT JOIN account a ON a.id = s.default_account_id
         LEFT JOIN LATERAL (
            SELECT id, label, amount, frequency, day_of_month, start_date
            FROM stream_schedule
            WHERE stream_id = s.id AND is_active = 1
            ORDER BY id ASC
            LIMIT 1
         ) ss ON true
         WHERE s.is_active = 1
         ORDER BY
            CASE
                WHEN s.kind = 'tmo_trust' THEN 0
                WHEN s.kind = 'credit_card' THEN 1
                WHEN s.kind = 'manual_income' THEN 2
                WHEN s.kind = 'manual_expense' THEN 3
                ELSE 4
            END,
            s.name ASC",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub async fn list_view_summaries(pool: &PgPool) -> Vec<StreamViewSummary> {
    sqlx::query_as(
        "SELECT id, name, description, is_default, is_active
         FROM stream_view
         WHERE is_active = 1
         ORDER BY is_default DESC, name ASC",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub async fn list_view_editors(pool: &PgPool) -> anyhow::Result<Vec<StreamViewEditor>> {
    let views = list_view_summaries(pool).await;
    let streams: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, name
         FROM stream
         WHERE is_active = 1
         ORDER BY name ASC",
    )
    .fetch_all(pool)
    .await?;
    let memberships: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT stream_view_id, stream_id
         FROM stream_view_stream",
    )
    .fetch_all(pool)
    .await?;

    let editors = views
        .into_iter()
        .map(|view| {
            let members = streams
                .iter()
                .map(|(stream_id, stream_name)| StreamViewMember {
                    stream_id: *stream_id,
                    stream_name: stream_name.clone(),
                    included: memberships.iter().any(|(view_id, member_stream_id)| {
                        *view_id == view.id && *member_stream_id == *stream_id
                    }),
                })
                .collect();

            StreamViewEditor {
                id: view.id,
                name: view.name,
                description: view.description,
                is_default: view.is_default,
                is_active: view.is_active,
                members,
            }
        })
        .collect();

    Ok(editors)
}

pub async fn default_view_id(pool: &PgPool) -> Option<i64> {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT id
         FROM stream_view
         WHERE is_default = 1 AND is_active = 1
         ORDER BY id ASC
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    row.map(|(id,)| id)
}

pub async fn create_stream(
    pool: &PgPool,
    name: &str,
    kind: &str,
    description: Option<&str>,
    default_account_id: Option<i64>,
    schedule_amount: Option<f64>,
    schedule_frequency: Option<&str>,
    due_day: Option<i32>,
    start_date: Option<&str>,
) -> anyhow::Result<i64> {
    let stream_type = match kind {
        "manual_income" => "manual_income",
        "manual_expense" => "manual_expense",
        "credit_card" => "credit_card_due",
        "tmo_trust" => "mortgage_portfolio",
        _ => "manual",
    };

    let (stream_id,): (i64,) = sqlx::query_as(
        "INSERT INTO stream (
            name, type, kind, description, default_account_id, is_active
         ) VALUES (
            $1, $2, $3, $4, $5, 1
         ) RETURNING id",
    )
    .bind(name.trim())
    .bind(stream_type)
    .bind(kind.trim())
    .bind(description.map(str::trim).filter(|value| !value.is_empty()))
    .bind(default_account_id)
    .fetch_one(pool)
    .await?;

    if let Some(view_id) = default_view_id(pool).await {
        sqlx::query(
            "INSERT INTO stream_view_stream (stream_view_id, stream_id)
             VALUES ($1, $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(view_id)
        .bind(stream_id)
        .execute(pool)
        .await?;
    }

    upsert_schedule(
        pool,
        stream_id,
        schedule_amount.unwrap_or(0.0),
        schedule_frequency,
        due_day,
        start_date,
        default_account_id,
        Some(name.trim()),
    )
    .await?;

    refresh_stream_schedule_events(pool).await?;

    Ok(stream_id)
}

pub async fn update_stream(
    pool: &PgPool,
    stream_id: i64,
    name: &str,
    kind: &str,
    description: Option<&str>,
    default_account_id: Option<i64>,
    schedule_amount: Option<f64>,
    schedule_frequency: Option<&str>,
    due_day: Option<i32>,
    start_date: Option<&str>,
) -> anyhow::Result<bool> {
    let stream_type = match kind {
        "manual_income" => "manual_income",
        "manual_expense" => "manual_expense",
        "credit_card" => "credit_card_due",
        "tmo_trust" => "mortgage_portfolio",
        _ => "manual",
    };

    let result = sqlx::query(
        "UPDATE stream
         SET name = $1,
             type = $2,
             kind = $3,
             description = $4,
             default_account_id = $5,
             updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
         WHERE id = $6",
    )
    .bind(name.trim())
    .bind(stream_type)
    .bind(kind.trim())
    .bind(description.map(str::trim).filter(|value| !value.is_empty()))
    .bind(default_account_id)
    .bind(stream_id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Ok(false);
    }

    upsert_schedule(
        pool,
        stream_id,
        schedule_amount.unwrap_or(0.0),
        schedule_frequency,
        due_day,
        start_date,
        default_account_id,
        Some(name.trim()),
    )
    .await?;

    if let Some(view_id) = default_view_id(pool).await {
        sqlx::query(
            "INSERT INTO stream_view_stream (stream_view_id, stream_id)
             VALUES ($1, $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(view_id)
        .bind(stream_id)
        .execute(pool)
        .await?;
    }

    refresh_stream_schedule_events(pool).await?;
    Ok(true)
}

async fn upsert_schedule(
    pool: &PgPool,
    stream_id: i64,
    amount: f64,
    schedule_frequency: Option<&str>,
    due_day: Option<i32>,
    start_date: Option<&str>,
    account_id: Option<i64>,
    fallback_label: Option<&str>,
) -> anyhow::Result<()> {
    let frequency = schedule_frequency
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match (frequency, due_day) {
        (Some("monthly"), Some(due_day)) => {
            let label = fallback_label
                .map(|value| format!("{value} due"))
                .unwrap_or_else(|| "Scheduled".to_string());
            let start_date = start_date
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    Utc::now()
                        .date_naive()
                        .with_day(1)
                        .unwrap_or_else(|| Utc::now().date_naive())
                        .to_string()
                });
            ensure_monthly_schedule(
                pool,
                stream_id,
                &label,
                amount,
                due_day,
                &start_date,
                account_id,
            )
            .await?;
        }
        _ => {
            sqlx::query(
                "UPDATE stream_schedule
                 SET is_active = 0,
                     updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
                 WHERE stream_id = $1",
            )
            .bind(stream_id)
            .execute(pool)
            .await?;
        }
    }

    Ok(())
}

pub async fn create_view(
    pool: &PgPool,
    name: &str,
    description: Option<&str>,
    stream_ids: &[i64],
) -> anyhow::Result<i64> {
    let (view_id,): (i64,) = sqlx::query_as(
        "INSERT INTO stream_view (
            name, description, is_default, is_active
         ) VALUES (
            $1, $2, 0, 1
         ) RETURNING id",
    )
    .bind(name.trim())
    .bind(description.map(str::trim).filter(|value| !value.is_empty()))
    .fetch_one(pool)
    .await?;

    replace_view_members(pool, view_id, stream_ids).await?;
    Ok(view_id)
}

pub async fn update_view(
    pool: &PgPool,
    view_id: i64,
    name: &str,
    description: Option<&str>,
    stream_ids: &[i64],
) -> anyhow::Result<bool> {
    let result = sqlx::query(
        "UPDATE stream_view
         SET name = $1,
             description = $2,
             updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
         WHERE id = $3",
    )
    .bind(name.trim())
    .bind(description.map(str::trim).filter(|value| !value.is_empty()))
    .bind(view_id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Ok(false);
    }

    replace_view_members(pool, view_id, stream_ids).await?;
    Ok(true)
}

async fn replace_view_members(
    pool: &PgPool,
    view_id: i64,
    stream_ids: &[i64],
) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM stream_view_stream WHERE stream_view_id = $1")
        .bind(view_id)
        .execute(pool)
        .await?;

    for stream_id in stream_ids {
        sqlx::query(
            "INSERT INTO stream_view_stream (stream_view_id, stream_id)
             VALUES ($1, $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(view_id)
        .bind(stream_id)
        .execute(pool)
        .await?;
    }

    Ok(())
}

pub async fn stream_exists(pool: &PgPool, id: i64) -> anyhow::Result<bool> {
    let exists: Option<(i64,)> = sqlx::query_as(
        "SELECT id
         FROM stream
         WHERE id = $1 AND is_active = 1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("checking stream existence")?;

    Ok(exists.is_some())
}

pub async fn view_exists(pool: &PgPool, id: i64) -> anyhow::Result<bool> {
    let exists: Option<(i64,)> = sqlx::query_as(
        "SELECT id
         FROM stream_view
         WHERE id = $1 AND is_active = 1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("checking view existence")?;

    Ok(exists.is_some())
}
