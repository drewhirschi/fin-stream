use anyhow::Context;
use sqlx::PgPool;

use crate::models::{AccountView, CashSourceView};

pub async fn ensure_primary_account(pool: &PgPool) -> anyhow::Result<i64> {
    let existing: Option<(i64, Option<f64>)> = sqlx::query_as(
        "SELECT id, balance
         FROM account
         WHERE is_primary = 1
         ORDER BY id ASC
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;

    if let Some((id, balance)) = existing {
        if balance.is_none() {
            let current_cash: Option<(String,)> =
                sqlx::query_as("SELECT value FROM settings WHERE key = 'current_cash'")
                    .fetch_optional(pool)
                    .await?;
            if let Some((value,)) = current_cash {
                if let Ok(parsed) = value.parse::<f64>() {
                    sqlx::query(
                        "UPDATE account
                         SET balance = $1,
                             source_type = COALESCE(source_type, 'manual'),
                             balance_updated_at = COALESCE(
                                balance_updated_at,
                                TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
                             )
                         WHERE id = $2",
                    )
                    .bind(parsed)
                    .bind(id)
                    .execute(pool)
                    .await?;
                }
            }
        }
        return Ok(id);
    }

    let current_cash: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'current_cash'")
            .fetch_optional(pool)
            .await?;
    let seeded_balance = current_cash
        .and_then(|(value,)| value.parse::<f64>().ok())
        .unwrap_or(0.0);

    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO account (
            name, kind, balance, source_type, is_primary, is_active, balance_updated_at
         ) VALUES (
            'Primary Cash', 'cash', $1, 'manual', 1, 1,
            TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
         )
         RETURNING id",
    )
    .bind(seeded_balance)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

pub async fn list_accounts(pool: &PgPool) -> Vec<AccountView> {
    sqlx::query_as(
        "SELECT id, name, kind, balance, source_type, source_ref, metadata,
                balance_updated_at, is_primary, is_active, notes
         FROM account
         WHERE is_active = 1
         ORDER BY is_primary DESC, name ASC",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub async fn create_account(
    pool: &PgPool,
    name: &str,
    kind: &str,
    balance: Option<f64>,
    is_primary: bool,
    notes: Option<&str>,
) -> anyhow::Result<i64> {
    if is_primary {
        sqlx::query("UPDATE account SET is_primary = 0 WHERE is_primary = 1")
            .execute(pool)
            .await?;
    }

    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO account (
            name, kind, balance, source_type, is_primary, is_active, notes, balance_updated_at
         ) VALUES (
            $1, $2, $3, 'manual', $4, 1, $5,
            CASE WHEN $3 IS NULL THEN NULL
                 ELSE TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
            END
         ) RETURNING id",
    )
    .bind(name.trim())
    .bind(kind.trim())
    .bind(balance)
    .bind(if is_primary { 1 } else { 0 })
    .bind(notes.map(str::trim).filter(|value| !value.is_empty()))
    .fetch_one(pool)
    .await?;

    if is_primary {
        let amount = balance.unwrap_or(0.0);
        set_primary_balance(pool, amount, "manual", None, None, None).await?;
    }

    Ok(id)
}

pub async fn update_account(
    pool: &PgPool,
    id: i64,
    name: &str,
    kind: &str,
    balance: Option<f64>,
    is_primary: bool,
    notes: Option<&str>,
) -> anyhow::Result<bool> {
    if is_primary {
        sqlx::query("UPDATE account SET is_primary = 0 WHERE is_primary = 1 AND id <> $1")
            .bind(id)
            .execute(pool)
            .await?;
    }

    let result = sqlx::query(
        "UPDATE account
         SET name = $1,
             kind = $2,
             balance = $3,
             is_primary = $4,
             notes = $5,
             balance_updated_at = CASE
                WHEN $3 IS NULL THEN balance_updated_at
                ELSE TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
             END,
             updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
         WHERE id = $6",
    )
    .bind(name.trim())
    .bind(kind.trim())
    .bind(balance)
    .bind(if is_primary { 1 } else { 0 })
    .bind(notes.map(str::trim).filter(|value| !value.is_empty()))
    .bind(id)
    .execute(pool)
    .await?;

    if result.rows_affected() > 0 && is_primary {
        if let Some(amount) = balance {
            set_primary_balance(pool, amount, "manual", None, None, None).await?;
        }
    }

    Ok(result.rows_affected() > 0)
}

pub async fn set_primary_balance(
    pool: &PgPool,
    amount: f64,
    source_type: &str,
    source_ref: Option<&str>,
    metadata: Option<&str>,
    updated_at: Option<&str>,
) -> anyhow::Result<()> {
    let account_id = ensure_primary_account(pool).await?;
    let timestamp = updated_at.unwrap_or("");

    sqlx::query(
        "UPDATE account
         SET balance = $1,
             source_type = $2,
             source_ref = $3,
             metadata = $4,
             balance_updated_at = CASE
                WHEN NULLIF($5, '') IS NOT NULL THEN $5
                ELSE TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
             END,
             updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
         WHERE id = $6",
    )
    .bind(amount)
    .bind(source_type)
    .bind(source_ref)
    .bind(metadata)
    .bind(timestamp)
    .bind(account_id)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('current_cash', $1)
         ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
    )
    .bind(amount.to_string())
    .execute(pool)
    .await?;

    if let Some(source_type) = Some(source_type) {
        let source_value = serde_json::json!({
            "source_type": source_type,
            "source_ref": source_ref,
            "metadata": metadata.and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok()),
            "updated_at": updated_at,
            "amount": amount,
        });

        sqlx::query(
            "INSERT INTO settings (key, value) VALUES ('balance_source', $1)
             ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
        )
        .bind(source_value.to_string())
        .execute(pool)
        .await?;
    }

    Ok(())
}

pub async fn get_cash_source(pool: &PgPool) -> Option<CashSourceView> {
    let primary: Option<AccountView> = sqlx::query_as(
        "SELECT id, name, kind, balance, source_type, source_ref, metadata,
                balance_updated_at, is_primary, is_active, notes
         FROM account
         WHERE is_primary = 1 AND is_active = 1
         ORDER BY id ASC
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    if let Some(account) = primary {
        if let Some(amount) = account.balance {
            let mut detail = match account.source_type.as_deref() {
                Some("monarch") => format!("Synced from Monarch for {}", account.name),
                Some("manual") => format!("Manual balance for {}", account.name),
                Some(other) => format!("{} balance for {}", other, account.name),
                None => format!("Balance for {}", account.name),
            };

            if let Some(metadata) = account.metadata.as_deref() {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(metadata) {
                    let pending = json.get("pending_total").and_then(|v| v.as_f64());
                    let reported = json.get("reported_balance").and_then(|v| v.as_f64());
                    if let (Some(reported), Some(pending)) = (reported, pending) {
                        detail = format!(
                            "{}. Reported ${:.2}, pending ${:.2}.",
                            detail, reported, pending
                        );
                    }
                }
            }

            return Some(CashSourceView {
                amount,
                account_name: Some(account.name),
                source_kind: account.source_type.unwrap_or_else(|| "manual".into()),
                detail,
                updated_at: account.balance_updated_at,
            });
        }
    }

    let current_cash: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'current_cash'")
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    let balance_source: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'balance_source'")
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();

    let amount = current_cash.and_then(|(value,)| value.parse::<f64>().ok())?;
    let mut source_kind = "manual".to_string();
    let mut detail = "Manual current cash balance".to_string();
    let mut updated_at = None;

    if let Some((value,)) = balance_source {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&value) {
            source_kind = json
                .get("source_type")
                .and_then(|v| v.as_str())
                .unwrap_or("manual")
                .to_string();
            updated_at = json
                .get("updated_at")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            detail = match source_kind.as_str() {
                "monarch" => "Synced from Monarch".to_string(),
                _ => "Manual current cash balance".to_string(),
            };
        } else if value.starts_with("monarch:") {
            source_kind = "monarch".into();
            detail = "Synced from Monarch".into();
        }
    }

    Some(CashSourceView {
        amount,
        account_name: None,
        source_kind,
        detail,
        updated_at,
    })
}

pub async fn primary_account_id(pool: &PgPool) -> anyhow::Result<i64> {
    let id = ensure_primary_account(pool).await?;
    Ok(id)
}

pub async fn account_exists(pool: &PgPool, id: i64) -> anyhow::Result<bool> {
    let exists: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM account WHERE id = $1 AND is_active = 1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .context("checking account existence")?;
    Ok(exists.is_some())
}
