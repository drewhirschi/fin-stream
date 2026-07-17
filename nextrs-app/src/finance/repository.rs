use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};

use anyhow::{Context, bail};
use libsql::{Connection, Row, TransactionBehavior, params, params::IntoParams};

use super::{
    AccountDraft, AccountView, AmountCertainty, BootstrapResult, CanvasStreamView, CashSourceView,
    Direction, EventDraft, EventPatch, EventStatus, EventView, FinanceError, FinanceResult,
    ForecastQuery, ForecastResponse, ForecastRow, ForecastRowWithBalance, IsoDate, Patch,
    ProjectionWindow, ScheduleDraft, ScheduleFrequency, StreamConfigView, StreamDraft,
    StreamScheduleView, StreamViewDraft, StreamViewEditor, StreamViewMember, StreamViewSummary,
};

pub async fn verify_foreign_keys(connection: &Connection) -> anyhow::Result<()> {
    let mut rows = connection
        .query("PRAGMA foreign_keys", ())
        .await
        .context("read SQLite foreign-key setting")?;
    let enabled = rows
        .next()
        .await
        .context("read SQLite foreign-key row")?
        .context("SQLite did not report its foreign-key setting")?
        .get::<i64>(0)
        .context("decode SQLite foreign-key setting")?;
    if enabled != 1 {
        bail!("SQLite foreign-key enforcement is disabled for this connection");
    }
    Ok(())
}

/// A short-lived repository facade over the connection supplied by AppContext.
/// It borrows rather than owns the handle so AppContext can later provide one
/// connection per operation without changing the domain API.
pub struct FinanceRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> FinanceRepository<'connection> {
    pub fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub async fn save_account(&self, draft: &AccountDraft) -> FinanceResult<i64> {
        validate_nonempty("account name", &draft.name)?;
        validate_nonempty("account kind", &draft.kind)?;
        validate_optional_balance(draft.balance, draft.balance_as_of_date)?;

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin account transaction")?;

        if let Some(id) = draft.id
            && !draft.is_primary
        {
            let mut rows = transaction
                .query(
                    "SELECT is_primary FROM account WHERE id = ?1 AND is_active = 1 LIMIT 1",
                    params![id],
                )
                .await
                .context("check account primary status")?;
            let Some(row) = rows.next().await.context("read account primary status")? else {
                return Err(FinanceError::not_found(format!("active account {id}")));
            };
            let is_primary = row.get::<i64>(0).context("decode account primary status")? == 1;
            drop(rows);
            if is_primary {
                return Err(FinanceError::conflict(
                    "the current primary account cannot be demoted; make another account primary instead",
                ));
            }
        }

        if draft.is_primary {
            transaction
                .execute(
                    "UPDATE account SET is_primary = 0, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                     WHERE is_primary = 1 AND (?1 IS NULL OR id <> ?1)",
                    params![draft.id],
                )
                .await
                .context("clear previous primary account")?;
        }

        let id = if let Some(id) = draft.id {
            let changed = transaction
                .execute(
                    "UPDATE account \
                     SET name = ?2, kind = ?3, balance = ?4, balance_as_of_date = ?5, \
                         source_type = CASE \
                             WHEN ?4 IS NULL THEN NULL \
                             WHEN balance IS ?4 AND balance_as_of_date IS ?5 THEN source_type \
                             ELSE 'manual' \
                         END, \
                         source_ref = CASE \
                             WHEN balance IS ?4 AND balance_as_of_date IS ?5 THEN source_ref \
                             ELSE NULL \
                         END, \
                         metadata = CASE \
                             WHEN balance IS ?4 AND balance_as_of_date IS ?5 THEN metadata \
                             ELSE NULL \
                         END, \
                         balance_updated_at = CASE \
                             WHEN ?4 IS NULL THEN NULL \
                             WHEN balance IS ?4 AND balance_as_of_date IS ?5 THEN balance_updated_at \
                             ELSE strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                         END, \
                         is_primary = ?6, notes = ?7, \
                         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                     WHERE id = ?1 AND is_active = 1",
                    params![
                        id,
                        draft.name.trim(),
                        draft.kind.trim(),
                        draft.balance,
                        draft.balance_as_of_date.map(|date| date.to_string()),
                        bool_i64(draft.is_primary),
                        clean_optional(&draft.notes),
                    ],
                )
                .await
                .context("update account")?;
            if changed != 1 {
                return Err(FinanceError::not_found(format!("active account {id}")));
            }
            id
        } else {
            query_insert_id(
                &transaction,
                "INSERT INTO account ( \
                    name, kind, balance, balance_as_of_date, source_type, \
                    balance_updated_at, is_primary, is_active, notes \
                 ) VALUES ( \
                    ?1, ?2, ?3, ?4, CASE WHEN ?3 IS NULL THEN NULL ELSE 'manual' END, \
                    CASE WHEN ?3 IS NULL THEN NULL ELSE strftime('%Y-%m-%dT%H:%M:%fZ', 'now') END, \
                    ?5, 1, ?6 \
                 ) RETURNING id",
                params![
                    draft.name.trim(),
                    draft.kind.trim(),
                    draft.balance,
                    draft.balance_as_of_date.map(|date| date.to_string()),
                    bool_i64(draft.is_primary),
                    clean_optional(&draft.notes),
                ],
                "insert account",
            )
            .await?
        };

        transaction
            .commit()
            .await
            .context("commit account transaction")?;
        Ok(id)
    }

    /// Ensure the UI has a primary cash account without inventing a confirmed
    /// `$0` balance. `NULL` remains the onboarding signal until explicitly set.
    pub async fn ensure_primary_account(&self) -> anyhow::Result<i64> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin primary-account transaction")?;
        let id = ensure_primary_account_on(&transaction).await?;
        transaction
            .commit()
            .await
            .context("commit primary-account transaction")?;
        Ok(id)
    }

    pub async fn set_starting_balance(
        &self,
        amount: f64,
        as_of_date: IsoDate,
        source_type: &str,
        source_ref: Option<&str>,
        metadata: Option<&str>,
    ) -> FinanceResult<()> {
        validate_finite("starting balance", amount)?;
        validate_nonempty("balance source type", source_type)?;
        validate_json("balance metadata", metadata)?;

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin starting-balance transaction")?;
        let account_id = ensure_primary_account_on(&transaction).await?;
        let changed = transaction
            .execute(
                "UPDATE account \
                 SET balance = ?2, balance_as_of_date = ?3, source_type = ?4, source_ref = ?5, \
                     metadata = ?6, balance_updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), \
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ?1 AND is_active = 1 AND is_primary = 1",
                params![
                    account_id,
                    amount,
                    as_of_date.to_string(),
                    source_type.trim(),
                    clean_str(source_ref),
                    metadata,
                ],
            )
            .await
            .context("set starting balance")?;
        if changed != 1 {
            return Err(FinanceError::conflict(
                "primary account disappeared while setting its balance",
            ));
        }
        transaction
            .commit()
            .await
            .context("commit starting-balance transaction")?;
        Ok(())
    }

    pub async fn list_accounts(&self) -> anyhow::Result<Vec<AccountView>> {
        let mut rows = self
            .connection
            .query(
                "SELECT id, name, kind, balance, balance_as_of_date, source_type, source_ref, \
                        metadata, balance_updated_at, is_primary, is_active, notes \
                 FROM account WHERE is_active = 1 \
                 ORDER BY is_primary DESC, name COLLATE NOCASE ASC, id ASC",
                (),
            )
            .await
            .context("list accounts")?;
        let mut accounts = Vec::new();
        while let Some(row) = rows.next().await.context("read account row")? {
            accounts.push(account_from_row(&row)?);
        }
        Ok(accounts)
    }

    pub async fn get_cash_source(&self) -> anyhow::Result<Option<CashSourceView>> {
        let Some(account) = primary_account_with_balance(self.connection).await? else {
            return Ok(None);
        };
        let amount = account
            .balance
            .context("primary balance row had no balance")?;
        let as_of_date = account
            .balance_as_of_date
            .clone()
            .context("primary balance row had no as-of date")?;
        let source_kind = account
            .source_type
            .clone()
            .unwrap_or_else(|| "manual".to_owned());
        let detail = match source_kind.as_str() {
            "monarch" => format!("Synced from Monarch for {}", account.name),
            "manual" => format!("Manual balance for {}", account.name),
            other => format!("{other} balance for {}", account.name),
        };
        Ok(Some(CashSourceView {
            amount,
            as_of_date,
            account_name: Some(account.name),
            source_kind,
            detail,
            updated_at: account.balance_updated_at,
        }))
    }
}

async fn ensure_primary_account_on(connection: &Connection) -> anyhow::Result<i64> {
    let mut rows = connection
        .query(
            "SELECT id FROM account \
             WHERE is_primary = 1 AND is_active = 1 ORDER BY id ASC LIMIT 1",
            (),
        )
        .await
        .context("find primary account")?;
    if let Some(row) = rows.next().await.context("read primary account")? {
        return row.get::<i64>(0).context("decode primary account id");
    }
    query_insert_id(
        connection,
        "INSERT INTO account (name, kind, is_primary, is_active) \
         VALUES ('Primary Cash', 'cash', 1, 1) RETURNING id",
        (),
        "insert primary account",
    )
    .await
}

async fn primary_account_with_balance(
    connection: &Connection,
) -> anyhow::Result<Option<AccountView>> {
    let mut rows = connection
        .query(
            "SELECT id, name, kind, balance, balance_as_of_date, source_type, source_ref, \
                    metadata, balance_updated_at, is_primary, is_active, notes \
             FROM account \
             WHERE is_primary = 1 AND is_active = 1 \
               AND balance IS NOT NULL AND balance_as_of_date IS NOT NULL \
             ORDER BY id ASC LIMIT 1",
            (),
        )
        .await
        .context("query primary cash anchor")?;
    rows.next()
        .await
        .context("read primary cash anchor")?
        .map(|row| account_from_row(&row))
        .transpose()
}

fn account_from_row(row: &Row) -> anyhow::Result<AccountView> {
    Ok(AccountView {
        id: row.get(0).context("decode account.id")?,
        name: row.get(1).context("decode account.name")?,
        kind: row.get(2).context("decode account.kind")?,
        balance: row.get(3).context("decode account.balance")?,
        balance_as_of_date: row.get(4).context("decode account.balance_as_of_date")?,
        source_type: row.get(5).context("decode account.source_type")?,
        source_ref: row.get(6).context("decode account.source_ref")?,
        metadata: row.get(7).context("decode account.metadata")?,
        balance_updated_at: row.get(8).context("decode account.balance_updated_at")?,
        is_primary: row.get(9).context("decode account.is_primary")?,
        is_active: row.get(10).context("decode account.is_active")?,
        notes: row.get(11).context("decode account.notes")?,
    })
}

fn bool_i64(value: bool) -> i64 {
    i64::from(value)
}

fn clean_optional(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn clean_str(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn validate_nonempty(label: &str, value: &str) -> FinanceResult<()> {
    if value.trim().is_empty() {
        return Err(FinanceError::validation(format!("{label} cannot be empty")));
    }
    Ok(())
}

fn validate_finite(label: &str, value: f64) -> FinanceResult<()> {
    if !value.is_finite() {
        return Err(FinanceError::validation(format!("{label} must be finite")));
    }
    Ok(())
}

fn validate_magnitude(label: &str, value: f64) -> FinanceResult<()> {
    validate_finite(label, value)?;
    if value < 0.0 {
        return Err(FinanceError::validation(format!(
            "{label} must be a non-negative magnitude"
        )));
    }
    Ok(())
}

fn validate_positive_magnitude(label: &str, value: f64) -> FinanceResult<()> {
    validate_magnitude(label, value)?;
    if value == 0.0 {
        return Err(FinanceError::validation(format!(
            "{label} must be greater than zero"
        )));
    }
    Ok(())
}

fn validate_optional_balance(
    balance: Option<f64>,
    as_of_date: Option<IsoDate>,
) -> FinanceResult<()> {
    if let Some(balance) = balance {
        validate_finite("account balance", balance)?;
    }
    if balance.is_some() != as_of_date.is_some() {
        return Err(FinanceError::validation(
            "account balance and balance as-of date must be set or cleared together",
        ));
    }
    Ok(())
}

fn validate_json(label: &str, value: Option<&str>) -> FinanceResult<()> {
    if let Some(value) = value {
        serde_json::from_str::<serde_json::Value>(value)
            .map_err(|_| FinanceError::validation(format!("{label} must be valid JSON")))?;
    }
    Ok(())
}

async fn query_insert_id(
    connection: &Connection,
    sql: &str,
    query_params: impl IntoParams,
    operation: &str,
) -> anyhow::Result<i64> {
    let mut rows = connection
        .query(sql, query_params)
        .await
        .with_context(|| operation.to_owned())?;
    rows.next()
        .await
        .with_context(|| format!("read {operation} result"))?
        .with_context(|| format!("{operation} returned no row"))?
        .get::<i64>(0)
        .with_context(|| format!("decode {operation} id"))
}

impl FinanceRepository<'_> {
    /// Insert or update a stream, replace its complete active schedule set,
    /// attach it to the default view, and refresh only that stream's projected
    /// source slots in one transaction.
    pub async fn save_stream(
        &self,
        draft: &StreamDraft,
        projection_window: ProjectionWindow,
    ) -> FinanceResult<i64> {
        validate_stream_draft(draft)?;

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin stream transaction")?;
        if let Some(account_id) = draft.default_account_id {
            require_active_account(&transaction, account_id).await?;
        }
        if let Some(parent_id) = draft.parent_id {
            require_active_stream(&transaction, parent_id).await?;
        }
        for schedule in &draft.schedules {
            if let Some(account_id) = schedule.account_id {
                require_active_account(&transaction, account_id).await?;
            }
        }

        let stream_id = if let Some(stream_id) = draft.id {
            let changed = transaction
                .execute(
                    "UPDATE stream \
                     SET name = ?2, type = ?3, kind = ?4, direction = ?5, amount_certainty = ?6, \
                         description = ?7, default_account_id = ?8, configuration = ?9, \
                         parent_id = ?10, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                     WHERE id = ?1 AND is_active = 1",
                    params![
                        stream_id,
                        draft.name.trim(),
                        draft.stream_type.trim(),
                        draft.kind.trim(),
                        draft.direction.as_str(),
                        draft.amount_certainty.as_str(),
                        clean_optional(&draft.description),
                        draft.default_account_id,
                        draft.configuration.clone(),
                        draft.parent_id,
                    ],
                )
                .await
                .context("update stream")?;
            if changed != 1 {
                return Err(FinanceError::not_found(format!(
                    "active stream {stream_id}"
                )));
            }
            stream_id
        } else {
            query_insert_id(
                &transaction,
                "INSERT INTO stream ( \
                    name, type, kind, direction, amount_certainty, description, \
                    default_account_id, configuration, parent_id, is_active \
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1) RETURNING id",
                params![
                    draft.name.trim(),
                    draft.stream_type.trim(),
                    draft.kind.trim(),
                    draft.direction.as_str(),
                    draft.amount_certainty.as_str(),
                    clean_optional(&draft.description),
                    draft.default_account_id,
                    draft.configuration.clone(),
                    draft.parent_id,
                ],
                "insert stream",
            )
            .await?
        };

        let mut retained_schedule_ids = HashSet::new();
        for schedule in &draft.schedules {
            let schedule_id = save_schedule_on(&transaction, stream_id, schedule).await?;
            if !retained_schedule_ids.insert(schedule_id) {
                return Err(FinanceError::validation(format!(
                    "schedule {schedule_id} appears more than once"
                )));
            }
        }

        let mut existing_rows = transaction
            .query(
                "SELECT id FROM stream_schedule WHERE stream_id = ?1 AND is_active = 1",
                params![stream_id],
            )
            .await
            .context("list prior stream schedules")?;
        let mut deactivate = Vec::new();
        while let Some(row) = existing_rows
            .next()
            .await
            .context("read prior stream schedule")?
        {
            let id = row.get::<i64>(0).context("decode stream schedule id")?;
            if !retained_schedule_ids.contains(&id) {
                deactivate.push(id);
            }
        }
        for schedule_id in deactivate {
            transaction
                .execute(
                    "UPDATE stream_schedule \
                     SET is_active = 0, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                     WHERE id = ?1 AND stream_id = ?2",
                    params![schedule_id, stream_id],
                )
                .await
                .context("deactivate removed stream schedule")?;
        }

        transaction
            .execute(
                "INSERT INTO stream_view_stream (stream_view_id, stream_id) \
                 SELECT id, ?1 FROM stream_view WHERE is_default = 1 AND is_active = 1 \
                 ON CONFLICT(stream_view_id, stream_id) DO NOTHING",
                params![stream_id],
            )
            .await
            .context("attach stream to default view")?;

        refresh_stream_schedule_events_on(&transaction, stream_id, &draft.name, projection_window)
            .await?;
        transaction
            .commit()
            .await
            .context("commit stream transaction")?;
        Ok(stream_id)
    }

    pub async fn refresh_stream_schedule_events(
        &self,
        stream_id: i64,
        window: ProjectionWindow,
    ) -> FinanceResult<usize> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin schedule refresh transaction")?;
        let stream_name = require_active_stream(&transaction, stream_id).await?;
        let count =
            refresh_stream_schedule_events_on(&transaction, stream_id, &stream_name, window)
                .await?;
        transaction
            .commit()
            .await
            .context("commit schedule refresh transaction")?;
        Ok(count)
    }

    pub async fn delete_stream(&self, stream_id: i64) -> FinanceResult<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin stream deletion transaction")?;
        let changed = transaction
            .execute(
                "UPDATE stream \
                 SET is_active = 0, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ?1 AND is_active = 1",
                params![stream_id],
            )
            .await
            .context("deactivate stream")?;
        if changed == 0 {
            transaction
                .rollback()
                .await
                .context("rollback missing stream")?;
            return Err(FinanceError::not_found(format!(
                "active stream {stream_id}"
            )));
        }
        transaction
            .execute(
                "UPDATE stream_schedule \
                 SET is_active = 0, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE stream_id = ?1 AND is_active = 1",
                params![stream_id],
            )
            .await
            .context("deactivate stream schedules")?;
        transaction
            .execute(
                "UPDATE stream_event \
                 SET is_excluded = 1, exclusion_reason = 'schedule', \
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE stream_id = ?1 AND source_type = 'stream_schedule' \
                   AND status <> 'received' AND is_excluded = 0",
                params![stream_id],
            )
            .await
            .context("exclude open projections for deleted stream")?;
        transaction
            .commit()
            .await
            .context("commit stream deletion transaction")?;
        Ok(())
    }

    pub async fn list_streams(&self) -> anyhow::Result<Vec<StreamConfigView>> {
        let mut schedule_rows = self
            .connection
            .query(
                "SELECT id, stream_id, account_id, label, amount, frequency, day_of_month, \
                        start_date, end_date, is_active, metadata \
                 FROM stream_schedule WHERE is_active = 1 ORDER BY stream_id ASC, id ASC",
                (),
            )
            .await
            .context("list active stream schedules")?;
        let mut schedules_by_stream: HashMap<i64, Vec<StreamScheduleView>> = HashMap::new();
        while let Some(row) = schedule_rows
            .next()
            .await
            .context("read active stream schedule")?
        {
            let schedule = schedule_from_row(&row)?;
            schedules_by_stream
                .entry(schedule.stream_id)
                .or_default()
                .push(schedule);
        }

        let mut rows = self
            .connection
            .query(
                "SELECT s.id, s.name, s.type, s.kind, s.direction, s.amount_certainty, \
                        s.description, s.is_active, s.default_account_id, a.name, \
                        s.configuration, s.parent_id \
                 FROM stream s \
                 LEFT JOIN account a ON a.id = s.default_account_id \
                 WHERE s.is_active = 1 \
                 ORDER BY CASE s.kind \
                    WHEN 'tmo_trust' THEN 0 WHEN 'credit_card' THEN 1 \
                    WHEN 'manual_income' THEN 2 WHEN 'manual_expense' THEN 3 ELSE 4 END, \
                    s.name COLLATE NOCASE ASC, s.id ASC",
                (),
            )
            .await
            .context("list active streams")?;
        let mut streams = Vec::new();
        while let Some(row) = rows.next().await.context("read active stream")? {
            let id = row.get::<i64>(0).context("decode stream.id")?;
            let schedules = schedules_by_stream.remove(&id).unwrap_or_default();
            let first = schedules.first();
            streams.push(StreamConfigView {
                id,
                name: row.get(1).context("decode stream.name")?,
                stream_type: row.get(2).context("decode stream.type")?,
                kind: row.get(3).context("decode stream.kind")?,
                direction: row.get(4).context("decode stream.direction")?,
                amount_certainty: row.get(5).context("decode stream.amount_certainty")?,
                description: row.get(6).context("decode stream.description")?,
                is_active: row.get(7).context("decode stream.is_active")?,
                default_account_id: row.get(8).context("decode stream.default_account_id")?,
                default_account_name: row.get(9).context("decode stream default account name")?,
                configuration: row.get(10).context("decode stream.configuration")?,
                parent_id: row.get(11).context("decode stream.parent_id")?,
                schedule_id: first.map(|schedule| schedule.id),
                schedule_label: first.and_then(|schedule| schedule.label.clone()),
                schedule_amount: first.map(|schedule| schedule.amount),
                schedule_frequency: first.map(|schedule| schedule.frequency.clone()),
                due_day: first.and_then(|schedule| schedule.day_of_month),
                schedule_start_date: first.map(|schedule| schedule.start_date.clone()),
                schedules,
            });
        }
        Ok(streams)
    }

    pub async fn list_canvas_streams(&self) -> anyhow::Result<Vec<CanvasStreamView>> {
        let mut rows = self
            .connection
            .query(
                "SELECT id, name, kind FROM stream WHERE is_active = 1 \
                 ORDER BY CASE kind \
                    WHEN 'tmo_trust' THEN 0 WHEN 'credit_card' THEN 1 \
                    WHEN 'manual_income' THEN 2 WHEN 'manual_expense' THEN 3 ELSE 4 END, \
                    name COLLATE NOCASE ASC, id ASC",
                (),
            )
            .await
            .context("list Canvas streams")?;
        let mut streams = Vec::new();
        while let Some(row) = rows.next().await.context("read Canvas stream")? {
            streams.push(CanvasStreamView {
                id: row.get(0).context("decode Canvas stream.id")?,
                name: row.get(1).context("decode Canvas stream.name")?,
                kind: row.get(2).context("decode Canvas stream.kind")?,
            });
        }
        Ok(streams)
    }
}

fn validate_stream_draft(draft: &StreamDraft) -> FinanceResult<()> {
    validate_nonempty("stream name", &draft.name)?;
    validate_nonempty("stream type", &draft.stream_type)?;
    validate_nonempty("stream kind", &draft.kind)?;
    validate_json("stream configuration", draft.configuration.as_deref())?;
    for schedule in &draft.schedules {
        validate_magnitude("schedule amount", schedule.amount)?;
        validate_json("schedule metadata", schedule.metadata.as_deref())?;
        if schedule.frequency == ScheduleFrequency::Monthly && schedule.day_of_month.is_none() {
            return Err(FinanceError::validation(
                "monthly schedules require a day of month",
            ));
        }
        if let Some(day) = schedule.day_of_month
            && !(1..=31).contains(&day)
        {
            return Err(FinanceError::validation(
                "schedule day of month must be between 1 and 31",
            ));
        }
        if schedule
            .end_date
            .is_some_and(|end_date| end_date < schedule.start_date)
        {
            return Err(FinanceError::validation(
                "schedule end date cannot precede its start date",
            ));
        }
    }
    Ok(())
}

async fn save_schedule_on(
    connection: &Connection,
    stream_id: i64,
    schedule: &ScheduleDraft,
) -> FinanceResult<i64> {
    if let Some(schedule_id) = schedule.id {
        let changed = connection
            .execute(
                "UPDATE stream_schedule \
                 SET account_id = ?3, label = ?4, amount = ?5, frequency = ?6, day_of_month = ?7, \
                     start_date = ?8, end_date = ?9, is_active = 1, metadata = ?10, \
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ?1 AND stream_id = ?2",
                params![
                    schedule_id,
                    stream_id,
                    schedule.account_id,
                    clean_optional(&schedule.label),
                    schedule.amount,
                    schedule.frequency.as_str(),
                    schedule.day_of_month.map(i64::from),
                    schedule.start_date.to_string(),
                    schedule.end_date.map(|date| date.to_string()),
                    schedule.metadata.clone(),
                ],
            )
            .await
            .context("update stream schedule")?;
        if changed != 1 {
            return Err(FinanceError::not_found(format!(
                "schedule {schedule_id} on stream {stream_id}"
            )));
        }
        return Ok(schedule_id);
    }
    query_insert_id(
        connection,
        "INSERT INTO stream_schedule ( \
            stream_id, account_id, label, amount, frequency, day_of_month, \
            start_date, end_date, is_active, metadata \
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9) RETURNING id",
        params![
            stream_id,
            schedule.account_id,
            clean_optional(&schedule.label),
            schedule.amount,
            schedule.frequency.as_str(),
            schedule.day_of_month.map(i64::from),
            schedule.start_date.to_string(),
            schedule.end_date.map(|date| date.to_string()),
            schedule.metadata.clone(),
        ],
        "insert stream schedule",
    )
    .await
    .map_err(FinanceError::from)
}

fn schedule_from_row(row: &Row) -> anyhow::Result<StreamScheduleView> {
    Ok(StreamScheduleView {
        id: row.get(0).context("decode stream_schedule.id")?,
        stream_id: row.get(1).context("decode stream_schedule.stream_id")?,
        account_id: row.get(2).context("decode stream_schedule.account_id")?,
        label: row.get(3).context("decode stream_schedule.label")?,
        amount: row.get(4).context("decode stream_schedule.amount")?,
        frequency: row.get(5).context("decode stream_schedule.frequency")?,
        day_of_month: row.get(6).context("decode stream_schedule.day_of_month")?,
        start_date: row.get(7).context("decode stream_schedule.start_date")?,
        end_date: row.get(8).context("decode stream_schedule.end_date")?,
        is_active: row.get(9).context("decode stream_schedule.is_active")?,
        metadata: row.get(10).context("decode stream_schedule.metadata")?,
    })
}

async fn require_active_account(connection: &Connection, account_id: i64) -> FinanceResult<()> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM account WHERE id = ?1 AND is_active = 1 LIMIT 1",
            params![account_id],
        )
        .await
        .context("look up account reference")?;
    if rows
        .next()
        .await
        .context("read account reference")?
        .is_none()
    {
        return Err(FinanceError::not_found(format!(
            "active account {account_id}"
        )));
    }
    Ok(())
}

async fn require_active_stream(connection: &Connection, stream_id: i64) -> FinanceResult<String> {
    let mut rows = connection
        .query(
            "SELECT name FROM stream WHERE id = ?1 AND is_active = 1 LIMIT 1",
            params![stream_id],
        )
        .await
        .context("look up stream reference")?;
    let Some(row) = rows.next().await.context("read stream reference")? else {
        return Err(FinanceError::not_found(format!(
            "active stream {stream_id}"
        )));
    };
    row.get(0)
        .context("decode stream reference name")
        .map_err(FinanceError::from)
}

async fn refresh_stream_schedule_events_on(
    connection: &Connection,
    stream_id: i64,
    stream_name: &str,
    window: ProjectionWindow,
) -> FinanceResult<usize> {
    // First tombstone only untouched open source slots. User overrides,
    // user exclusions, and received rows remain durable even when a schedule
    // changes. Slots still generated below are reactivated by the upsert.
    connection
        .execute(
            "UPDATE stream_event \
             SET is_excluded = 1, exclusion_reason = 'schedule', \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE stream_id = ?1 AND source_type = 'stream_schedule' \
               AND expected_date BETWEEN ?2 AND ?3 \
               AND status IN ('projected', 'confirmed') \
               AND override_date IS NULL AND override_amount IS NULL \
               AND has_label_override = 0 AND has_account_override = 0 \
               AND (exclusion_reason IS NULL OR exclusion_reason = 'schedule')",
            params![
                stream_id,
                window.from.to_string(),
                window.through.to_string()
            ],
        )
        .await
        .context("tombstone obsolete schedule projections")?;

    let mut rows = connection
        .query(
            "SELECT id, stream_id, account_id, label, amount, frequency, day_of_month, \
                    start_date, end_date, is_active, metadata \
             FROM stream_schedule \
             WHERE stream_id = ?1 AND is_active = 1 ORDER BY id ASC",
            params![stream_id],
        )
        .await
        .context("load schedules for projection refresh")?;
    let mut schedules = Vec::new();
    while let Some(row) = rows.next().await.context("read schedule for refresh")? {
        schedules.push(schedule_from_row(&row)?);
    }

    let mut projected = 0;
    for schedule in schedules {
        let frequency = ScheduleFrequency::from_str(&schedule.frequency)
            .context("decode schedule frequency")?;
        let start_date =
            IsoDate::from_str(&schedule.start_date).context("decode schedule start date")?;
        let end_date = schedule
            .end_date
            .as_deref()
            .map(IsoDate::from_str)
            .transpose()
            .context("decode schedule end date")?;
        let range_start = start_date.max(window.from);
        let range_end = end_date.unwrap_or(window.through).min(window.through);
        if range_end < range_start {
            continue;
        }
        let occurrences = schedule_occurrences(
            frequency,
            start_date,
            range_start,
            range_end,
            schedule.day_of_month.map(|day| day as u8),
        )?;

        for occurrence in occurrences {
            // Keep one stable source slot per cadence period. A due-day edit
            // updates January's January row instead of creating a duplicate;
            // override columns on that row remain authoritative.
            let slot = projection_slot(frequency, start_date, occurrence);
            let source_id = format!("stream_schedule:{}:{slot}", schedule.id);
            let label = schedule
                .label
                .clone()
                .unwrap_or_else(|| format!("{stream_name} due"));
            let metadata = serde_json::json!({
                "schedule_id": schedule.id,
                "stream_name": stream_name,
            })
            .to_string();
            connection
                .execute(
                    "INSERT INTO stream_event ( \
                        stream_id, account_id, label, expected_date, amount, status, \
                        source_id, source_type, metadata, is_excluded, exclusion_reason \
                     ) VALUES (?1, ?2, ?3, ?4, ?5, 'projected', ?6, 'stream_schedule', ?7, 0, NULL) \
                     ON CONFLICT(stream_id, source_type, source_id) DO UPDATE SET \
                        account_id = excluded.account_id, label = excluded.label, \
                        expected_date = excluded.expected_date, amount = excluded.amount, \
                        metadata = excluded.metadata, \
                        is_excluded = CASE \
                            WHEN stream_event.exclusion_reason = 'user' THEN 1 ELSE 0 END, \
                        exclusion_reason = CASE \
                            WHEN stream_event.exclusion_reason = 'user' THEN 'user' ELSE NULL END, \
                        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                     WHERE stream_event.status IN ('projected', 'confirmed')",
                    params![
                        stream_id,
                        schedule.account_id,
                        label,
                        occurrence.to_string(),
                        schedule.amount,
                        source_id,
                        metadata,
                    ],
                )
                .await
                .context("upsert projected schedule event")?;
            projected += 1;
        }
    }
    Ok(projected)
}

fn projection_slot(frequency: ScheduleFrequency, anchor: IsoDate, occurrence: IsoDate) -> String {
    match frequency {
        ScheduleFrequency::Monthly => {
            format!("monthly:{:04}-{:02}", occurrence.year(), occurrence.month())
        }
        ScheduleFrequency::Semimonthly => {
            let half = if occurrence.day() == 15 { "mid" } else { "end" };
            format!(
                "semimonthly:{:04}-{:02}:{half}",
                occurrence.year(),
                occurrence.month()
            )
        }
        ScheduleFrequency::Weekly => {
            format!("weekly:{}", anchor.days_until(occurrence) / 7)
        }
        ScheduleFrequency::Biweekly => {
            format!("biweekly:{}", anchor.days_until(occurrence) / 14)
        }
        ScheduleFrequency::Annual => format!("annual:{:04}", occurrence.year()),
        ScheduleFrequency::OneTime => "one_time".to_owned(),
    }
}

fn schedule_occurrences(
    frequency: ScheduleFrequency,
    anchor: IsoDate,
    from: IsoDate,
    through: IsoDate,
    day_of_month: Option<u8>,
) -> anyhow::Result<Vec<IsoDate>> {
    if through < from {
        return Ok(Vec::new());
    }
    match frequency {
        ScheduleFrequency::Monthly => {
            let day = day_of_month.context("monthly schedule is missing a day of month")?;
            let mut month = from.first_of_month();
            let mut dates = Vec::new();
            while month <= through {
                let candidate = month.with_day_clamped(day);
                if candidate >= anchor && candidate >= from && candidate <= through {
                    dates.push(candidate);
                }
                month = month.next_month()?;
            }
            Ok(dates)
        }
        ScheduleFrequency::Semimonthly => {
            let mut month = from.first_of_month();
            let mut dates = Vec::new();
            while month <= through {
                for candidate in [month.with_day_clamped(15), month.last_of_month()] {
                    if candidate >= anchor && candidate >= from && candidate <= through {
                        dates.push(candidate);
                    }
                }
                month = month.next_month()?;
            }
            Ok(dates)
        }
        ScheduleFrequency::Weekly => step_occurrences(anchor, from, through, 7),
        ScheduleFrequency::Biweekly => step_occurrences(anchor, from, through, 14),
        ScheduleFrequency::Annual => {
            let mut dates = Vec::new();
            for year in from.year()..=through.year() {
                let candidate = anchor.in_year_clamped(year)?;
                if candidate >= anchor && candidate >= from && candidate <= through {
                    dates.push(candidate);
                }
            }
            Ok(dates)
        }
        ScheduleFrequency::OneTime => {
            if anchor >= from && anchor <= through {
                Ok(vec![anchor])
            } else {
                Ok(Vec::new())
            }
        }
    }
}

fn step_occurrences(
    anchor: IsoDate,
    from: IsoDate,
    through: IsoDate,
    step_days: i64,
) -> anyhow::Result<Vec<IsoDate>> {
    let mut cursor = if anchor < from {
        let distance = anchor.days_until(from);
        let steps = (distance + step_days - 1) / step_days;
        anchor.add_days(steps * step_days)?
    } else {
        anchor
    };
    let mut dates = Vec::new();
    while cursor <= through {
        dates.push(cursor);
        cursor = cursor.add_days(step_days)?;
    }
    Ok(dates)
}

impl FinanceRepository<'_> {
    pub async fn save_view(&self, draft: &StreamViewDraft) -> FinanceResult<i64> {
        validate_nonempty("stream view name", &draft.name)?;
        let unique_stream_ids: HashSet<i64> = draft.stream_ids.iter().copied().collect();
        if unique_stream_ids.len() != draft.stream_ids.len() {
            return Err(FinanceError::validation(
                "stream view members contain duplicate stream ids",
            ));
        }

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin stream-view transaction")?;
        for stream_id in &draft.stream_ids {
            require_active_stream(&transaction, *stream_id).await?;
        }
        if draft.is_default {
            transaction
                .execute(
                    "UPDATE stream_view \
                     SET is_default = 0, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                     WHERE is_default = 1 AND (?1 IS NULL OR id <> ?1)",
                    params![draft.id],
                )
                .await
                .context("clear previous default stream view")?;
        }
        let view_id = if let Some(view_id) = draft.id {
            let changed = transaction
                .execute(
                    "UPDATE stream_view \
                     SET name = ?2, description = ?3, is_default = ?4, \
                         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                     WHERE id = ?1 AND is_active = 1",
                    params![
                        view_id,
                        draft.name.trim(),
                        clean_optional(&draft.description),
                        bool_i64(draft.is_default),
                    ],
                )
                .await
                .context("update stream view")?;
            if changed != 1 {
                return Err(FinanceError::not_found(format!(
                    "active stream view {view_id}"
                )));
            }
            view_id
        } else {
            query_insert_id(
                &transaction,
                "INSERT INTO stream_view (name, description, is_default, is_active) \
                 VALUES (?1, ?2, ?3, 1) RETURNING id",
                params![
                    draft.name.trim(),
                    clean_optional(&draft.description),
                    bool_i64(draft.is_default),
                ],
                "insert stream view",
            )
            .await?
        };
        transaction
            .execute(
                "DELETE FROM stream_view_stream WHERE stream_view_id = ?1",
                params![view_id],
            )
            .await
            .context("clear stream-view membership")?;
        for stream_id in &draft.stream_ids {
            transaction
                .execute(
                    "INSERT INTO stream_view_stream (stream_view_id, stream_id) VALUES (?1, ?2)",
                    params![view_id, stream_id],
                )
                .await
                .context("insert stream-view member")?;
        }
        transaction
            .commit()
            .await
            .context("commit stream-view transaction")?;
        Ok(view_id)
    }

    pub async fn list_view_summaries(&self) -> anyhow::Result<Vec<StreamViewSummary>> {
        let mut rows = self
            .connection
            .query(
                "SELECT id, name, description, is_default, is_active \
                 FROM stream_view WHERE is_active = 1 \
                 ORDER BY is_default DESC, name COLLATE NOCASE ASC, id ASC",
                (),
            )
            .await
            .context("list stream views")?;
        let mut views = Vec::new();
        while let Some(row) = rows.next().await.context("read stream view")? {
            views.push(StreamViewSummary {
                id: row.get(0).context("decode stream_view.id")?,
                name: row.get(1).context("decode stream_view.name")?,
                description: row.get(2).context("decode stream_view.description")?,
                is_default: row.get(3).context("decode stream_view.is_default")?,
                is_active: row.get(4).context("decode stream_view.is_active")?,
            });
        }
        Ok(views)
    }

    pub async fn list_view_editors(&self) -> anyhow::Result<Vec<StreamViewEditor>> {
        let views = self.list_view_summaries().await?;
        let mut stream_rows = self
            .connection
            .query(
                "SELECT id, name FROM stream WHERE is_active = 1 \
                 ORDER BY name COLLATE NOCASE ASC, id ASC",
                (),
            )
            .await
            .context("list streams for view editor")?;
        let mut streams = Vec::new();
        while let Some(row) = stream_rows
            .next()
            .await
            .context("read stream for view editor")?
        {
            streams.push((
                row.get::<i64>(0).context("decode view-editor stream id")?,
                row.get::<String>(1)
                    .context("decode view-editor stream name")?,
            ));
        }
        let mut membership_rows = self
            .connection
            .query(
                "SELECT stream_view_id, stream_id FROM stream_view_stream",
                (),
            )
            .await
            .context("list stream-view memberships")?;
        let mut memberships = HashSet::new();
        while let Some(row) = membership_rows
            .next()
            .await
            .context("read stream-view membership")?
        {
            memberships.insert((
                row.get::<i64>(0).context("decode membership view id")?,
                row.get::<i64>(1).context("decode membership stream id")?,
            ));
        }
        Ok(views
            .into_iter()
            .map(|view| StreamViewEditor {
                id: view.id,
                name: view.name,
                description: view.description,
                is_default: view.is_default,
                is_active: view.is_active,
                members: streams
                    .iter()
                    .map(|(stream_id, stream_name)| StreamViewMember {
                        stream_id: *stream_id,
                        stream_name: stream_name.clone(),
                        included: memberships.contains(&(view.id, *stream_id)),
                    })
                    .collect(),
            })
            .collect())
    }

    pub async fn default_view_id(&self) -> anyhow::Result<Option<i64>> {
        let mut rows = self
            .connection
            .query(
                "SELECT id FROM stream_view \
                 WHERE is_default = 1 AND is_active = 1 ORDER BY id ASC LIMIT 1",
                (),
            )
            .await
            .context("query default stream view")?;
        rows.next()
            .await
            .context("read default stream view")?
            .map(|row| row.get::<i64>(0).context("decode default stream view id"))
            .transpose()
    }
}

impl FinanceRepository<'_> {
    pub async fn create_manual_event(&self, draft: &EventDraft) -> FinanceResult<i64> {
        validate_nonempty("event label", &draft.label)?;
        validate_positive_magnitude("event amount", draft.amount)?;
        validate_json("event metadata", draft.metadata.as_deref())?;
        if draft.status == EventStatus::Received {
            return Err(FinanceError::validation(
                "a received event must be created by reconciliation with actual fields",
            ));
        }

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin manual-event transaction")?;
        require_active_stream(&transaction, draft.stream_id).await?;
        if let Some(account_id) = draft.account_id {
            require_active_account(&transaction, account_id).await?;
        }
        let event_id = query_insert_id(
            &transaction,
            "INSERT INTO stream_event ( \
                stream_id, account_id, label, expected_date, amount, status, \
                source_id, source_type, metadata, notes \
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, 'manual', ?7, ?8) RETURNING id",
            params![
                draft.stream_id,
                draft.account_id,
                draft.label.trim(),
                draft.expected_date.to_string(),
                draft.amount,
                draft.status.as_str(),
                draft.metadata.clone(),
                clean_optional(&draft.notes),
            ],
            "insert manual event",
        )
        .await?;
        let source_id = format!("manual:{event_id}");
        transaction
            .execute(
                "UPDATE stream_event SET source_id = ?2 WHERE id = ?1",
                params![event_id, source_id],
            )
            .await
            .context("assign manual event source id")?;
        transaction
            .commit()
            .await
            .context("commit manual-event transaction")?;
        Ok(event_id)
    }

    /// Apply user overrides atomically. Generated source values remain intact,
    /// so clearing an override returns to the schedule seed. Received events and
    /// provider-derived rows are immutable through this user-facing API.
    pub async fn patch_event(&self, event_id: i64, patch: &EventPatch) -> FinanceResult<()> {
        if let Patch::Set(amount) = &patch.amount {
            validate_positive_magnitude("event amount override", *amount)?;
        }
        if let Patch::Set(label) = &patch.label {
            validate_nonempty("event label override", label)?;
        }
        if let Patch::Set(account_id) = &patch.account_id {
            require_active_account(self.connection, *account_id).await?;
        }

        let (label_code, label_value) = patch_text_parts(&patch.label);
        let (date_code, date_value) = patch.expected_date.as_parts(|date| date.to_string());
        let (amount_code, amount_value) = patch.amount.as_copy_parts();
        let (account_code, account_value) = patch.account_id.as_copy_parts();
        let (notes_code, notes_value) = patch_text_parts(&patch.notes);

        let changed = self
            .connection
            .execute(
                "UPDATE stream_event SET \
                    override_label = CASE WHEN ?2 = 0 THEN override_label ELSE ?3 END, \
                    has_label_override = CASE WHEN ?2 = 0 THEN has_label_override ELSE 1 END, \
                    override_date = CASE WHEN ?4 = 0 THEN override_date ELSE ?5 END, \
                    override_amount = CASE WHEN ?6 = 0 THEN override_amount ELSE ?7 END, \
                    override_account_id = CASE \
                        WHEN ?8 = 0 THEN override_account_id \
                        WHEN ?8 = 1 THEN NULL \
                        ELSE ?9 END, \
                    has_account_override = CASE \
                        WHEN ?8 = 0 THEN has_account_override \
                        WHEN ?8 = 1 THEN 0 \
                        ELSE 1 END, \
                    notes = CASE WHEN ?10 = 0 THEN notes ELSE ?11 END, \
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ?1 AND status IN ('projected', 'confirmed') \
                   AND source_type IN ('manual', 'stream_schedule')",
                params![
                    event_id,
                    label_code,
                    label_value,
                    date_code,
                    date_value,
                    amount_code,
                    amount_value,
                    account_code,
                    account_value,
                    notes_code,
                    notes_value,
                ],
            )
            .await
            .context("patch event overrides")?;
        if changed == 1 {
            Ok(())
        } else {
            immutable_or_missing_event(self.connection, event_id, "edit").await
        }
    }

    /// Remove a manual event physically, or retain a durable user tombstone for
    /// a generated schedule event so the next refresh cannot recreate it.
    pub async fn remove_event(&self, event_id: i64) -> FinanceResult<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin event-removal transaction")?;
        let deleted = transaction
            .execute(
                "DELETE FROM stream_event \
                 WHERE id = ?1 AND source_type = 'manual' \
                   AND status IN ('projected', 'confirmed')",
                params![event_id],
            )
            .await
            .context("delete manual event")?;
        let changed = if deleted == 1 {
            true
        } else {
            transaction
                .execute(
                    "UPDATE stream_event \
                     SET is_excluded = 1, exclusion_reason = 'user', \
                         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                     WHERE id = ?1 AND source_type = 'stream_schedule' \
                       AND status IN ('projected', 'confirmed')",
                    params![event_id],
                )
                .await
                .context("exclude generated event")?
                == 1
        };
        transaction
            .commit()
            .await
            .context("commit event-removal transaction")?;
        if changed {
            Ok(())
        } else {
            immutable_or_missing_event(self.connection, event_id, "remove").await
        }
    }

    pub async fn restore_scheduled_event(&self, event_id: i64) -> FinanceResult<()> {
        let changed = self
            .connection
            .execute(
                "UPDATE stream_event \
                 SET is_excluded = 0, exclusion_reason = NULL, \
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ?1 AND source_type = 'stream_schedule' \
                   AND status IN ('projected', 'confirmed') \
                   AND exclusion_reason = 'user'",
                params![event_id],
            )
            .await
            .context("restore generated event")?;
        if changed == 1 {
            Ok(())
        } else {
            immutable_or_missing_event(self.connection, event_id, "restore").await
        }
    }

    /// Collapse expectation into reality without destroying the expected date
    /// or amount. Only canonical user/schedule expectations are accepted here;
    /// integration reconciliation gets a separate repository boundary later.
    pub async fn reconcile_event(
        &self,
        event_id: i64,
        actual_date: IsoDate,
        actual_amount: f64,
    ) -> FinanceResult<()> {
        validate_positive_magnitude("actual event amount", actual_amount)?;
        let changed = self
            .connection
            .execute(
                "UPDATE stream_event \
                 SET actual_date = ?2, actual_amount = ?3, status = 'received', \
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ?1 AND status IN ('projected', 'confirmed') \
                   AND is_excluded = 0 \
                   AND source_type IN ('manual', 'stream_schedule')",
                params![event_id, actual_date.to_string(), actual_amount],
            )
            .await
            .context("reconcile expected event")?;
        if changed == 1 {
            Ok(())
        } else {
            immutable_or_missing_event(self.connection, event_id, "reconcile").await
        }
    }

    pub async fn list_events(
        &self,
        from: IsoDate,
        through: IsoDate,
        stream_id: Option<i64>,
        include_excluded: bool,
    ) -> anyhow::Result<Vec<EventView>> {
        if through < from {
            bail!("event range ends before it starts");
        }
        let mut rows = self
            .connection
            .query(
                "WITH effective_event AS ( \
                    SELECT e.*, \
                        CASE WHEN e.has_label_override = 1 THEN e.override_label ELSE e.label END AS effective_label, \
                        COALESCE(e.actual_date, e.override_date, e.expected_date) AS effective_date, \
                        COALESCE(e.actual_amount, e.override_amount, e.amount) AS effective_amount, \
                        CASE WHEN e.has_account_override = 1 THEN e.override_account_id ELSE e.account_id END AS effective_account_id \
                    FROM stream_event e \
                 ) \
                 SELECT id, stream_id, effective_account_id, effective_label, expected_date, amount, \
                        override_label, override_date, override_amount, override_account_id, \
                        has_account_override, actual_date, actual_amount, effective_date, effective_amount, \
                        status, is_excluded, source_id, source_type, metadata, notes \
                 FROM effective_event \
                 WHERE effective_date BETWEEN ?1 AND ?2 \
                   AND (?3 IS NULL OR stream_id = ?3) \
                   AND (?4 = 1 OR is_excluded = 0) \
                 ORDER BY effective_date ASC, id ASC",
                params![
                    from.to_string(),
                    through.to_string(),
                    stream_id,
                    bool_i64(include_excluded),
                ],
            )
            .await
            .context("list stream events")?;
        let mut events = Vec::new();
        while let Some(row) = rows.next().await.context("read stream event")? {
            events.push(event_from_row(&row)?);
        }
        Ok(events)
    }
}

trait PatchParts<T> {
    fn as_parts<U>(&self, map: impl FnOnce(&T) -> U) -> (i64, Option<U>);
    fn as_copy_parts(&self) -> (i64, Option<T>)
    where
        T: Copy;
}

impl<T> PatchParts<T> for Patch<T> {
    fn as_parts<U>(&self, map: impl FnOnce(&T) -> U) -> (i64, Option<U>) {
        match self {
            Patch::Keep => (0, None),
            Patch::Clear => (1, None),
            Patch::Set(value) => (2, Some(map(value))),
        }
    }

    fn as_copy_parts(&self) -> (i64, Option<T>)
    where
        T: Copy,
    {
        self.as_parts(|value| *value)
    }
}

fn patch_text_parts(patch: &Patch<String>) -> (i64, Option<String>) {
    patch.as_parts(|value| value.trim().to_owned())
}

fn event_from_row(row: &Row) -> anyhow::Result<EventView> {
    Ok(EventView {
        id: row.get(0).context("decode stream_event.id")?,
        stream_id: row.get(1).context("decode stream_event.stream_id")?,
        account_id: row.get(2).context("decode effective event account")?,
        label: row.get(3).context("decode effective event label")?,
        expected_date: row.get(4).context("decode stream_event.expected_date")?,
        amount: row.get(5).context("decode stream_event.amount")?,
        override_label: row.get(6).context("decode stream_event.override_label")?,
        override_date: row.get(7).context("decode stream_event.override_date")?,
        override_amount: row.get(8).context("decode stream_event.override_amount")?,
        override_account_id: row
            .get(9)
            .context("decode stream_event.override_account_id")?,
        has_account_override: row
            .get::<i64>(10)
            .context("decode stream_event.has_account_override")?
            == 1,
        actual_date: row.get(11).context("decode stream_event.actual_date")?,
        actual_amount: row.get(12).context("decode stream_event.actual_amount")?,
        effective_date: row.get(13).context("decode effective event date")?,
        effective_amount: row.get(14).context("decode effective event amount")?,
        status: row.get(15).context("decode stream_event.status")?,
        is_excluded: row
            .get::<i64>(16)
            .context("decode stream_event.is_excluded")?
            == 1,
        source_id: row.get(17).context("decode stream_event.source_id")?,
        source_type: row.get(18).context("decode stream_event.source_type")?,
        metadata: row.get(19).context("decode stream_event.metadata")?,
        notes: row.get(20).context("decode stream_event.notes")?,
    })
}

async fn immutable_or_missing_event(
    connection: &Connection,
    event_id: i64,
    operation: &str,
) -> FinanceResult<()> {
    let mut rows = connection
        .query(
            "SELECT status, source_type, is_excluded FROM stream_event WHERE id = ?1 LIMIT 1",
            params![event_id],
        )
        .await
        .context("classify rejected event mutation")?;
    let Some(row) = rows.next().await.context("read rejected event mutation")? else {
        return Err(FinanceError::not_found(format!("event {event_id}")));
    };
    let status = row
        .get::<String>(0)
        .context("decode rejected event status")?;
    let source_type = row
        .get::<Option<String>>(1)
        .context("decode rejected event source type")?
        .unwrap_or_else(|| "unknown".to_owned());
    let is_excluded = row
        .get::<i64>(2)
        .context("decode rejected event exclusion")?
        == 1;
    Err(FinanceError::conflict(format!(
        "cannot {operation} event {event_id} (status={status}, source_type={source_type}, excluded={is_excluded})"
    )))
}

impl FinanceRepository<'_> {
    /// Compute a deterministic forecast around the primary account's explicit
    /// cash anchor. The query expands to include every event between the anchor
    /// and the requested window, so a future-only or past-only window still has
    /// correct balances.
    pub async fn compute_forecast(
        &self,
        query: ForecastQuery,
    ) -> anyhow::Result<Option<ForecastResponse>> {
        if query.through < query.from {
            bail!("forecast range ends before it starts");
        }
        let Some(account) = primary_account_with_balance(self.connection).await? else {
            return Ok(None);
        };
        let starting_balance = account.balance.context("cash anchor has no balance")?;
        let anchor_date = account
            .balance_as_of_date
            .as_deref()
            .context("cash anchor has no as-of date")?
            .parse::<IsoDate>()
            .context("cash anchor contains an invalid as-of date")?;
        let cash_source = self
            .get_cash_source()
            .await?
            .context("cash anchor disappeared during forecast")?;

        let envelope_from = query.from.min(anchor_date);
        let envelope_through = query.through.max(anchor_date);
        let events = self
            .forecast_events(
                envelope_from,
                envelope_through,
                query.stream_id,
                query.view_id,
            )
            .await?;
        let signed: Vec<f64> = events
            .iter()
            .map(|event| event.direction.signed(event.row.amount))
            .collect();
        let opening_balance =
            balance_before(starting_balance, anchor_date, query.from, &events, &signed);

        let mut balances = vec![starting_balance; events.len()];
        let mut future_balance = starting_balance;
        for (index, event) in events.iter().enumerate() {
            if event.date > anchor_date {
                future_balance += signed[index];
                balances[index] = future_balance;
            }
        }
        let mut past_balance = starting_balance;
        for index in (0..events.len()).rev() {
            if events[index].date <= anchor_date {
                balances[index] = past_balance;
                past_balance -= signed[index];
            }
        }

        let ending_balance = balance_at(
            starting_balance,
            anchor_date,
            query.through,
            &events,
            &signed,
        );
        let rows = events
            .into_iter()
            .enumerate()
            .filter(|(_, event)| event.date >= query.from && event.date <= query.through)
            .map(|(index, event)| {
                let is_late = event.row.actual_date.is_none()
                    && event.date < query.today
                    && matches!(event.row.status.as_str(), "projected" | "confirmed");
                ForecastRowWithBalance {
                    event_id: event.row.event_id,
                    stream_id: event.row.stream_id,
                    account_id: event.row.account_id,
                    has_account_override: event.row.has_account_override,
                    date: event.row.date,
                    expected_date: event.row.expected_date,
                    actual_date: event.row.actual_date,
                    label: event.row.label,
                    stream_name: event.row.stream_name,
                    account_name: event.row.account_name,
                    amount: signed[index],
                    running_balance: balances[index],
                    status: event.row.status,
                    direction: event.row.direction,
                    amount_certainty: event.row.amount_certainty,
                    source_type: event.row.source_type,
                    metadata: event.row.metadata,
                    is_late,
                }
            })
            .collect();
        Ok(Some(ForecastResponse {
            starting_balance,
            balance_as_of_date: anchor_date.to_string(),
            cash_source,
            opening_balance,
            rows,
            ending_balance,
        }))
    }

    async fn forecast_events(
        &self,
        from: IsoDate,
        through: IsoDate,
        stream_id: Option<i64>,
        view_id: Option<i64>,
    ) -> anyhow::Result<Vec<ParsedForecastEvent>> {
        let mut rows = self
            .connection
            .query(
                "WITH effective_event AS ( \
                    SELECT e.id, e.stream_id, \
                        CASE WHEN e.has_account_override = 1 \
                            THEN e.override_account_id \
                            ELSE COALESCE(e.account_id, s.default_account_id) END AS account_id, \
                        COALESCE(e.actual_date, e.override_date, e.expected_date) AS effective_date, \
                        e.expected_date, e.actual_date, \
                        CASE WHEN e.has_label_override = 1 THEN e.override_label ELSE e.label END AS effective_label, \
                        s.name AS stream_name, \
                        COALESCE(e.actual_amount, e.override_amount, e.amount) AS effective_amount, \
                        e.status, s.direction, s.amount_certainty, e.source_type, e.metadata, \
                        e.has_account_override \
                    FROM stream_event e \
                    JOIN stream s ON s.id = e.stream_id \
                    WHERE s.is_active = 1 AND e.is_excluded = 0 \
                 ) \
                 SELECT ee.id, ee.stream_id, ee.account_id, ee.effective_date, ee.expected_date, \
                        ee.actual_date, ee.effective_label, ee.stream_name, a.name, \
                        ee.effective_amount, ee.status, ee.direction, ee.amount_certainty, \
                        ee.source_type, ee.metadata, ee.has_account_override \
                 FROM effective_event ee \
                 LEFT JOIN account a ON a.id = ee.account_id \
                 WHERE ee.effective_date BETWEEN ?1 AND ?2 \
                   AND (?3 IS NULL OR ee.stream_id = ?3) \
                   AND ( \
                        ?4 IS NULL OR EXISTS ( \
                            SELECT 1 FROM stream_view_stream svs \
                            JOIN stream_view sv ON sv.id = svs.stream_view_id \
                            WHERE svs.stream_view_id = ?4 AND svs.stream_id = ee.stream_id \
                              AND sv.is_active = 1 \
                        ) \
                   ) \
                 ORDER BY ee.effective_date ASC, ee.id ASC",
                params![
                    from.to_string(),
                    through.to_string(),
                    stream_id,
                    view_id,
                ],
            )
            .await
            .context("query forecast events")?;
        let mut events = Vec::new();
        while let Some(row) = rows.next().await.context("read forecast event")? {
            let date_text = row
                .get::<String>(3)
                .context("decode forecast effective date")?;
            let date = date_text
                .parse::<IsoDate>()
                .context("forecast event has an invalid effective date")?;
            let direction_text = row.get::<String>(11).context("decode forecast direction")?;
            let direction = Direction::from_str(&direction_text)
                .context("forecast event has an invalid direction")?;
            let certainty = row
                .get::<String>(12)
                .context("decode forecast amount certainty")?;
            AmountCertainty::from_str(&certainty)
                .context("forecast event has invalid amount certainty")?;
            events.push(ParsedForecastEvent {
                date,
                direction,
                row: ForecastRow {
                    event_id: row.get(0).context("decode forecast event id")?,
                    stream_id: row.get(1).context("decode forecast stream id")?,
                    account_id: row.get(2).context("decode forecast account id")?,
                    has_account_override: row
                        .get::<i64>(15)
                        .context("decode forecast account override flag")?
                        == 1,
                    date: date_text,
                    expected_date: row.get(4).context("decode forecast expected date")?,
                    actual_date: row.get(5).context("decode forecast actual date")?,
                    label: row.get(6).context("decode forecast label")?,
                    stream_name: row.get(7).context("decode forecast stream name")?,
                    account_name: row.get(8).context("decode forecast account name")?,
                    amount: row.get(9).context("decode forecast amount")?,
                    status: row.get(10).context("decode forecast status")?,
                    direction: direction_text,
                    amount_certainty: certainty,
                    source_type: row.get(13).context("decode forecast source type")?,
                    metadata: row.get(14).context("decode forecast metadata")?,
                },
            });
        }
        Ok(events)
    }
}

struct ParsedForecastEvent {
    date: IsoDate,
    direction: Direction,
    row: ForecastRow,
}

fn balance_at(
    starting_balance: f64,
    anchor: IsoDate,
    target: IsoDate,
    events: &[ParsedForecastEvent],
    signed: &[f64],
) -> f64 {
    if target >= anchor {
        starting_balance
            + events
                .iter()
                .zip(signed)
                .filter(|(event, _)| event.date > anchor && event.date <= target)
                .map(|(_, amount)| amount)
                .sum::<f64>()
    } else {
        starting_balance
            - events
                .iter()
                .zip(signed)
                .filter(|(event, _)| event.date > target && event.date <= anchor)
                .map(|(_, amount)| amount)
                .sum::<f64>()
    }
}

fn balance_before(
    starting_balance: f64,
    anchor: IsoDate,
    window_start: IsoDate,
    events: &[ParsedForecastEvent],
    signed: &[f64],
) -> f64 {
    if window_start > anchor {
        starting_balance
            + events
                .iter()
                .zip(signed)
                .filter(|(event, _)| event.date > anchor && event.date < window_start)
                .map(|(_, amount)| amount)
                .sum::<f64>()
    } else {
        starting_balance
            - events
                .iter()
                .zip(signed)
                .filter(|(event, _)| event.date >= window_start && event.date <= anchor)
                .map(|(_, amount)| amount)
                .sum::<f64>()
    }
}

impl FinanceRepository<'_> {
    /// Explicit, idempotent application bootstrap. This is intentionally not a
    /// cold-start side effect; invoke it from an operator/bootstrap command.
    pub async fn bootstrap_defaults(&self, today: IsoDate) -> FinanceResult<BootstrapResult> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin default-configuration transaction")?;
        let primary_account_id = ensure_primary_account_on(&transaction).await?;

        let seeds = [
            (
                "Trust Deeds",
                "mortgage_portfolio",
                "tmo_trust",
                Direction::In,
                AmountCertainty::Known,
                "Payments flowing into your trust deed cash view.",
            ),
            (
                "One-off Income",
                "manual_income",
                "manual_income",
                Direction::In,
                AmountCertainty::Known,
                "Manual one-time inflows.",
            ),
            (
                "One-off Expense",
                "manual_expense",
                "manual_expense",
                Direction::Out,
                AmountCertainty::Known,
                "Manual one-time outflows and bills.",
            ),
            (
                "Hannah Costco",
                "credit_card_due",
                "credit_card",
                Direction::Out,
                AmountCertainty::Estimated,
                "Monthly Hannah Costco payment due date.",
            ),
            (
                "Drew Costco",
                "credit_card_due",
                "credit_card",
                Direction::Out,
                AmountCertainty::Estimated,
                "Monthly Drew Costco payment due date.",
            ),
            (
                "Apple Card",
                "credit_card_due",
                "credit_card",
                Direction::Out,
                AmountCertainty::Estimated,
                "Monthly Apple Card payment due date.",
            ),
        ];
        let mut stream_ids = Vec::new();
        for &(name, stream_type, kind, direction, certainty, description) in &seeds {
            stream_ids.push(
                ensure_seed_stream_on(
                    &transaction,
                    name,
                    stream_type,
                    kind,
                    direction,
                    certainty,
                    description,
                    primary_account_id,
                )
                .await?,
            );
        }

        for (index, day) in [(3_usize, 21_u8), (4, 22), (5, 31)] {
            ensure_seed_monthly_schedule_on(
                &transaction,
                stream_ids[index],
                seeds[index].0,
                day,
                today.first_of_month(),
                primary_account_id,
            )
            .await?;
        }
        let default_view_id = ensure_default_view_on(&transaction).await?;
        transaction
            .execute(
                "INSERT INTO stream_view_stream (stream_view_id, stream_id) \
                 SELECT ?1, id FROM stream WHERE is_active = 1 \
                 ON CONFLICT(stream_view_id, stream_id) DO NOTHING",
                params![default_view_id],
            )
            .await
            .context("populate default stream view")?;

        let through = today.add_days(365).map_err(FinanceError::from)?;
        let window = ProjectionWindow::new(today, through).map_err(FinanceError::from)?;
        for stream_id in &stream_ids {
            let stream_name = require_active_stream(&transaction, *stream_id).await?;
            refresh_stream_schedule_events_on(&transaction, *stream_id, &stream_name, window)
                .await?;
        }
        transaction
            .commit()
            .await
            .context("commit default-configuration transaction")?;
        Ok(BootstrapResult {
            primary_account_id,
            default_view_id,
            stream_ids,
        })
    }
}

#[allow(clippy::too_many_arguments)]
async fn ensure_seed_stream_on(
    connection: &Connection,
    name: &str,
    stream_type: &str,
    kind: &str,
    direction: Direction,
    certainty: AmountCertainty,
    description: &str,
    account_id: i64,
) -> anyhow::Result<i64> {
    let mut rows = connection
        .query(
            "SELECT id FROM stream WHERE name = ?1 ORDER BY id ASC LIMIT 1",
            params![name],
        )
        .await
        .context("find seeded stream")?;
    if let Some(row) = rows.next().await.context("read seeded stream")? {
        let id = row.get::<i64>(0).context("decode seeded stream id")?;
        connection
            .execute(
                "UPDATE stream \
                 SET name = ?2, type = ?3, kind = ?4, direction = ?5, amount_certainty = ?6, \
                     description = ?7, default_account_id = ?8, is_active = 1, \
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ?1",
                params![
                    id,
                    name,
                    stream_type,
                    kind,
                    direction.as_str(),
                    certainty.as_str(),
                    description,
                    account_id,
                ],
            )
            .await
            .context("update seeded stream")?;
        return Ok(id);
    }
    query_insert_id(
        connection,
        "INSERT INTO stream ( \
            name, type, kind, direction, amount_certainty, description, default_account_id \
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id",
        params![
            name,
            stream_type,
            kind,
            direction.as_str(),
            certainty.as_str(),
            description,
            account_id,
        ],
        "insert seeded stream",
    )
    .await
}

async fn ensure_seed_monthly_schedule_on(
    connection: &Connection,
    stream_id: i64,
    stream_name: &str,
    day: u8,
    start_date: IsoDate,
    account_id: i64,
) -> anyhow::Result<i64> {
    let mut rows = connection
        .query(
            "SELECT id FROM stream_schedule WHERE stream_id = ?1 ORDER BY id ASC LIMIT 1",
            params![stream_id],
        )
        .await
        .context("find seeded stream schedule")?;
    if let Some(row) = rows.next().await.context("read seeded stream schedule")? {
        let id = row.get::<i64>(0).context("decode seeded schedule id")?;
        connection
            .execute(
                "UPDATE stream_schedule \
                 SET account_id = ?2, label = ?3, amount = 0.0, frequency = 'monthly', \
                     day_of_month = ?4, start_date = ?5, end_date = NULL, is_active = 1, \
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ?1",
                params![
                    id,
                    account_id,
                    format!("{stream_name} due"),
                    i64::from(day),
                    start_date.to_string(),
                ],
            )
            .await
            .context("update seeded stream schedule")?;
        return Ok(id);
    }
    query_insert_id(
        connection,
        "INSERT INTO stream_schedule ( \
            stream_id, account_id, label, amount, frequency, day_of_month, start_date \
         ) VALUES (?1, ?2, ?3, 0.0, 'monthly', ?4, ?5) RETURNING id",
        params![
            stream_id,
            account_id,
            format!("{stream_name} due"),
            i64::from(day),
            start_date.to_string(),
        ],
        "insert seeded stream schedule",
    )
    .await
}

async fn ensure_default_view_on(connection: &Connection) -> anyhow::Result<i64> {
    let mut rows = connection
        .query(
            "SELECT id FROM stream_view \
             WHERE is_default = 1 OR name = 'All Streams' \
             ORDER BY is_default DESC, id ASC LIMIT 1",
            (),
        )
        .await
        .context("find default stream view")?;
    if let Some(row) = rows.next().await.context("read default stream view")? {
        let id = row.get::<i64>(0).context("decode default stream view id")?;
        connection
            .execute(
                "UPDATE stream_view SET is_default = 0 \
                 WHERE is_default = 1 AND id <> ?1",
                params![id],
            )
            .await
            .context("clear duplicate default stream view")?;
        connection
            .execute(
                "UPDATE stream_view \
                 SET name = 'All Streams', description = 'Merged view across every active stream.', \
                     is_default = 1, is_active = 1, \
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ?1",
                params![id],
            )
            .await
            .context("update default stream view")?;
        return Ok(id);
    }
    query_insert_id(
        connection,
        "INSERT INTO stream_view (name, description, is_default, is_active) \
         VALUES ('All Streams', 'Merged view across every active stream.', 1, 1) RETURNING id",
        (),
        "insert default stream view",
    )
    .await
}
