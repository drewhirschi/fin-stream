use std::str::FromStr;

use anyhow::Context;
use libsql::{Connection, Row, Rows, TransactionBehavior, params};

use super::{
    ClaimOutcome, OperationControl, OperationError, OperationMode, OperationResult, SyncCompletion,
    SyncRun, SyncRunStatus,
};

const STALE_EXECUTION_MESSAGE: &str = "execution exceeded platform maximum duration";

/// Durable coordination for inline serverless work.
///
/// Coordinator flow:
/// stale interruption -> atomic claim -> provider work outside the database ->
/// conditional completion. The unique indexes, rather than isolate-local
/// state, provide the one-running and scheduled-slot guarantees.
pub struct OperationRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> OperationRepository<'connection> {
    pub fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub async fn control(&self) -> OperationResult<OperationControl> {
        control_on(self.connection).await
    }

    pub async fn set_control(
        &self,
        mode: OperationMode,
        scheduler_enabled: bool,
        updated_at: &str,
    ) -> OperationResult<OperationControl> {
        require_timestamp("operation-control timestamp", updated_at)?;
        if mode == OperationMode::ReadOnly && scheduler_enabled {
            return Err(OperationError::validation(
                "the scheduler cannot be enabled while operations are read-only",
            ));
        }

        let mut rows = self
            .connection
            .query(
                "UPDATE operation_control \
                 SET mode = ?1, scheduler_enabled = ?2, updated_at = ?3 \
                 WHERE id = 1 \
                 RETURNING mode, scheduler_enabled, updated_at",
                params![mode.as_str(), bool_i64(scheduler_enabled), updated_at],
            )
            .await
            .context("update operation control")?;
        let Some(row) = rows
            .next()
            .await
            .context("read updated operation control")?
        else {
            return Err(OperationError::coordination(
                "operation-control singleton disappeared during update",
            ));
        };
        control_from_row(&row)
    }

    /// Enter maintenance mode and stop scheduled execution in one singleton
    /// update. Keeping this transition indivisible prevents a scheduler-on
    /// value from surviving a cutover freeze.
    pub async fn enter_read_only(&self, updated_at: &str) -> OperationResult<OperationControl> {
        self.set_control(OperationMode::ReadOnly, false, updated_at)
            .await
    }

    /// Enable browser/manual writes while leaving scheduled work off. Cron is
    /// an explicit, later operator transition during cutover.
    pub async fn enable_writes(&self, updated_at: &str) -> OperationResult<OperationControl> {
        self.set_control(OperationMode::Enabled, false, updated_at)
            .await
    }

    /// Toggle scheduled execution without changing the write mode. Enabling
    /// is conditional in the UPDATE itself, so it cannot race a read-only
    /// transition into an invalid state.
    pub async fn set_scheduler_enabled(
        &self,
        enabled: bool,
        updated_at: &str,
    ) -> OperationResult<OperationControl> {
        require_timestamp("operation-control timestamp", updated_at)?;
        let mut rows = self
            .connection
            .query(
                "UPDATE operation_control \
                 SET scheduler_enabled = ?1, updated_at = ?2 \
                 WHERE id = 1 AND (?1 = 0 OR mode = 'enabled') \
                 RETURNING mode, scheduler_enabled, updated_at",
                params![bool_i64(enabled), updated_at],
            )
            .await
            .context("update operation scheduler control")?;
        if let Some(row) = rows
            .next()
            .await
            .context("read updated operation scheduler control")?
        {
            return control_from_row(&row);
        }
        drop(rows);

        let control = self.control().await?;
        if enabled && control.mode == OperationMode::ReadOnly {
            Err(OperationError::ReadOnly)
        } else {
            Err(OperationError::coordination(
                "operation scheduler control was not updated",
            ))
        }
    }

    pub async fn require_writes_enabled(&self) -> OperationResult<OperationControl> {
        let control = self.control().await?;
        if control.mode != OperationMode::Enabled {
            return Err(OperationError::ReadOnly);
        }
        Ok(control)
    }

    pub async fn require_scheduler_enabled(&self) -> OperationResult<OperationControl> {
        let control = self.require_writes_enabled().await?;
        if !control.scheduler_enabled {
            return Err(OperationError::SchedulerDisabled);
        }
        Ok(control)
    }

    /// Claim a user-triggered run. The single INSERT is both the write guard
    /// and the one-running lock; a connection can never overlap itself.
    pub async fn claim_manual(
        &self,
        connection_slug: &str,
        started_at: &str,
    ) -> OperationResult<ClaimOutcome> {
        require_nonempty("connection slug", connection_slug)?;
        require_timestamp("run start timestamp", started_at)?;

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin manual sync claim transaction")?;
        let mut rows = transaction
            .query(
                "INSERT INTO sync_log (connection_slug, scheduled_for, started_at, status) \
                 SELECT ?1, NULL, ?2, 'running' \
                 FROM operation_control \
                 WHERE id = 1 AND mode = 'enabled' \
                   AND EXISTS ( \
                       SELECT 1 FROM intg_integration_connection WHERE slug = ?1 \
                   ) \
                 ON CONFLICT DO NOTHING \
                 RETURNING id, connection_slug, scheduled_for, started_at, finished_at, status, \
                           error_message, endpoints_hit, events_upserted, loans_upserted, \
                           snapshots_created",
                params![connection_slug, started_at],
            )
            .await
            .context("claim manual sync run")?;
        if let Some(row) = rows.next().await.context("read manual sync claim")? {
            let claimed = sync_run_from_row(&row)?;
            drop(row);
            drop(rows);
            transaction
                .commit()
                .await
                .context("commit manual sync claim")?;
            return Ok(ClaimOutcome::Claimed(claimed));
        }
        drop(rows);

        require_writes_enabled_on(&transaction).await?;
        require_connection_on(&transaction, connection_slug).await?;
        let outcome = if let Some(current) = current_run_on(&transaction, connection_slug).await? {
            ClaimOutcome::AlreadyRunning(current)
        } else {
            return Err(OperationError::coordination(
                "manual claim was not inserted, but no running conflict remains",
            ));
        };
        transaction
            .commit()
            .await
            .context("commit manual sync conflict classification")?;
        Ok(outcome)
    }

    /// Claim a deterministic cron slot. A later successful manual/scheduled
    /// run covers the slot, and the slot's unique index prevents redelivery
    /// after either success or failure.
    pub async fn claim_scheduled(
        &self,
        connection_slug: &str,
        scheduled_for: &str,
        started_at: &str,
    ) -> OperationResult<ClaimOutcome> {
        require_nonempty("connection slug", connection_slug)?;
        require_timestamp("scheduled slot timestamp", scheduled_for)?;
        require_timestamp("run start timestamp", started_at)?;

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin scheduled sync claim transaction")?;
        let mut rows = transaction
            .query(
                "INSERT INTO sync_log (connection_slug, scheduled_for, started_at, status) \
                 SELECT ?1, ?2, ?3, 'running' \
                 FROM operation_control \
                 WHERE id = 1 AND mode = 'enabled' AND scheduler_enabled = 1 \
                   AND EXISTS ( \
                       SELECT 1 FROM intg_integration_connection WHERE slug = ?1 \
                   ) \
                   AND NOT EXISTS ( \
                       SELECT 1 FROM sync_log \
                       WHERE connection_slug = ?1 \
                         AND status = 'success' \
                         AND started_at >= ?2 \
                   ) \
                 ON CONFLICT DO NOTHING \
                 RETURNING id, connection_slug, scheduled_for, started_at, finished_at, status, \
                           error_message, endpoints_hit, events_upserted, loans_upserted, \
                           snapshots_created",
                params![connection_slug, scheduled_for, started_at],
            )
            .await
            .context("claim scheduled sync run")?;
        if let Some(row) = rows.next().await.context("read scheduled sync claim")? {
            let claimed = sync_run_from_row(&row)?;
            drop(row);
            drop(rows);
            transaction
                .commit()
                .await
                .context("commit scheduled sync claim")?;
            return Ok(ClaimOutcome::Claimed(claimed));
        }
        drop(rows);

        require_scheduler_enabled_on(&transaction).await?;
        require_connection_on(&transaction, connection_slug).await?;
        let outcome = if let Some(existing) =
            run_for_scheduled_slot_on(&transaction, connection_slug, scheduled_for).await?
        {
            ClaimOutcome::AlreadyScheduled(existing)
        } else if let Some(coverage) =
            successful_run_since_on(&transaction, connection_slug, scheduled_for).await?
        {
            ClaimOutcome::CoveredBySuccess(coverage)
        } else if let Some(current) = current_run_on(&transaction, connection_slug).await? {
            ClaimOutcome::AlreadyRunning(current)
        } else {
            return Err(OperationError::coordination(
                "scheduled claim was not inserted, but no durable conflict remains",
            ));
        };
        transaction
            .commit()
            .await
            .context("commit scheduled sync conflict classification")?;
        Ok(outcome)
    }

    /// Mark invocations that are provably older than the caller's hard
    /// platform-duration-plus-skew cutoff. The cutoff is deliberately supplied
    /// by the caller; this repository does not guess deployment limits.
    pub async fn interrupt_stale(
        &self,
        finished_at: &str,
        cutoff_started_at: &str,
    ) -> OperationResult<Vec<SyncRun>> {
        require_timestamp("stale-run finish timestamp", finished_at)?;
        require_timestamp("stale-run cutoff timestamp", cutoff_started_at)?;
        self.require_writes_enabled().await?;

        let rows = self
            .connection
            .query(
                "UPDATE sync_log \
                 SET status = 'error', finished_at = ?1, error_message = ?2 \
                 WHERE status = 'running' AND started_at < ?3 \
                   AND EXISTS ( \
                       SELECT 1 FROM operation_control WHERE id = 1 AND mode = 'enabled' \
                   ) \
                 RETURNING id, connection_slug, scheduled_for, started_at, finished_at, status, \
                           error_message, endpoints_hit, events_upserted, loans_upserted, \
                           snapshots_created",
                params![finished_at, STALE_EXECUTION_MESSAGE, cutoff_started_at],
            )
            .await
            .context("interrupt stale sync runs")?;
        let interrupted = collect_runs(rows, "stale sync transition").await?;
        if interrupted.is_empty() {
            // If the mode changed between the guard and UPDATE, surface the
            // fail-closed state instead of silently reporting no stale work.
            self.require_writes_enabled().await?;
        }
        Ok(interrupted)
    }

    pub async fn complete_success(
        &self,
        run_id: i64,
        finished_at: &str,
        completion: &SyncCompletion,
    ) -> OperationResult<SyncRun> {
        require_positive_id("sync run", run_id)?;
        require_timestamp("run finish timestamp", finished_at)?;
        validate_completion(completion)?;

        let mut rows = self
            .connection
            .query(
                "UPDATE sync_log \
                 SET status = 'success', finished_at = ?2, error_message = NULL, \
                     endpoints_hit = ?3, events_upserted = ?4, loans_upserted = ?5, \
                     snapshots_created = ?6 \
                 WHERE id = ?1 AND status = 'running' \
                 RETURNING id, connection_slug, scheduled_for, started_at, finished_at, status, \
                           error_message, endpoints_hit, events_upserted, loans_upserted, \
                           snapshots_created",
                params![
                    run_id,
                    finished_at,
                    completion.endpoints_hit.clone(),
                    completion.events_upserted,
                    completion.loans_upserted,
                    completion.snapshots_created,
                ],
            )
            .await
            .context("complete successful sync run")?;
        let Some(row) = rows.next().await.context("read successful completion")? else {
            return Err(OperationError::coordination(format!(
                "sync run {run_id} was not running during successful completion"
            )));
        };
        sync_run_from_row(&row)
    }

    pub async fn complete_error(
        &self,
        run_id: i64,
        finished_at: &str,
        error_message: &str,
    ) -> OperationResult<SyncRun> {
        require_positive_id("sync run", run_id)?;
        require_timestamp("run finish timestamp", finished_at)?;
        require_nonempty("sync error message", error_message)?;

        let mut rows = self
            .connection
            .query(
                "UPDATE sync_log \
                 SET status = 'error', finished_at = ?2, error_message = ?3 \
                 WHERE id = ?1 AND status = 'running' \
                 RETURNING id, connection_slug, scheduled_for, started_at, finished_at, status, \
                           error_message, endpoints_hit, events_upserted, loans_upserted, \
                           snapshots_created",
                params![run_id, finished_at, error_message],
            )
            .await
            .context("complete failed sync run")?;
        let Some(row) = rows.next().await.context("read failed completion")? else {
            return Err(OperationError::coordination(format!(
                "sync run {run_id} was not running during error completion"
            )));
        };
        sync_run_from_row(&row)
    }

    pub async fn current_run(&self, connection_slug: &str) -> OperationResult<Option<SyncRun>> {
        require_nonempty("connection slug", connection_slug)?;
        current_run_on(self.connection, connection_slug).await
    }

    pub async fn sync_run(&self, run_id: i64) -> OperationResult<Option<SyncRun>> {
        require_positive_id("sync run", run_id)?;
        let rows = self
            .connection
            .query(
                "SELECT id, connection_slug, scheduled_for, started_at, finished_at, status, \
                        error_message, endpoints_hit, events_upserted, loans_upserted, \
                        snapshots_created \
                 FROM sync_log WHERE id = ?1",
                params![run_id],
            )
            .await
            .context("query sync run")?;
        one_run(rows, "sync run").await
    }

    pub async fn list_recent(
        &self,
        connection_slug: &str,
        limit: u16,
    ) -> OperationResult<Vec<SyncRun>> {
        require_nonempty("connection slug", connection_slug)?;
        if limit == 0 || limit > 500 {
            return Err(OperationError::validation(
                "sync-run list limit must be between 1 and 500",
            ));
        }
        let rows = self
            .connection
            .query(
                "SELECT id, connection_slug, scheduled_for, started_at, finished_at, status, \
                        error_message, endpoints_hit, events_upserted, loans_upserted, \
                        snapshots_created \
                 FROM sync_log WHERE connection_slug = ?1 \
                 ORDER BY started_at DESC, id DESC LIMIT ?2",
                params![connection_slug, i64::from(limit)],
            )
            .await
            .context("query recent sync runs")?;
        collect_runs(rows, "recent sync run").await
    }
}

async fn control_on(connection: &Connection) -> OperationResult<OperationControl> {
    let mut rows = connection
        .query(
            "SELECT mode, scheduler_enabled, updated_at \
             FROM operation_control WHERE id = 1",
            (),
        )
        .await
        .context("query operation control")?;
    let Some(row) = rows.next().await.context("read operation control")? else {
        return Err(OperationError::coordination(
            "operation-control singleton is missing",
        ));
    };
    control_from_row(&row)
}

async fn require_writes_enabled_on(connection: &Connection) -> OperationResult<OperationControl> {
    let control = control_on(connection).await?;
    if control.mode != OperationMode::Enabled {
        return Err(OperationError::ReadOnly);
    }
    Ok(control)
}

async fn require_scheduler_enabled_on(
    connection: &Connection,
) -> OperationResult<OperationControl> {
    let control = require_writes_enabled_on(connection).await?;
    if !control.scheduler_enabled {
        return Err(OperationError::SchedulerDisabled);
    }
    Ok(control)
}

async fn require_connection_on(connection: &Connection, slug: &str) -> OperationResult<()> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM intg_integration_connection WHERE slug = ?1 LIMIT 1",
            params![slug],
        )
        .await
        .context("check integration connection")?;
    if rows
        .next()
        .await
        .context("read integration connection check")?
        .is_none()
    {
        return Err(OperationError::ConnectionNotFound(slug.to_owned()));
    }
    Ok(())
}

async fn current_run_on(
    connection: &Connection,
    connection_slug: &str,
) -> OperationResult<Option<SyncRun>> {
    let rows = connection
        .query(
            "SELECT id, connection_slug, scheduled_for, started_at, finished_at, status, \
                    error_message, endpoints_hit, events_upserted, loans_upserted, \
                    snapshots_created \
             FROM sync_log \
             WHERE connection_slug = ?1 AND status = 'running' \
             ORDER BY started_at DESC, id DESC LIMIT 1",
            params![connection_slug],
        )
        .await
        .context("query current sync run")?;
    one_run(rows, "current sync run").await
}

async fn run_for_scheduled_slot_on(
    connection: &Connection,
    slug: &str,
    scheduled_for: &str,
) -> OperationResult<Option<SyncRun>> {
    let rows = connection
        .query(
            "SELECT id, connection_slug, scheduled_for, started_at, finished_at, status, \
                    error_message, endpoints_hit, events_upserted, loans_upserted, \
                    snapshots_created \
             FROM sync_log \
             WHERE connection_slug = ?1 AND scheduled_for = ?2 \
             ORDER BY id DESC LIMIT 1",
            params![slug, scheduled_for],
        )
        .await
        .context("query scheduled sync slot")?;
    one_run(rows, "scheduled sync slot").await
}

async fn successful_run_since_on(
    connection: &Connection,
    slug: &str,
    scheduled_for: &str,
) -> OperationResult<Option<SyncRun>> {
    let rows = connection
        .query(
            "SELECT id, connection_slug, scheduled_for, started_at, finished_at, status, \
                    error_message, endpoints_hit, events_upserted, loans_upserted, \
                    snapshots_created \
             FROM sync_log \
             WHERE connection_slug = ?1 AND status = 'success' AND started_at >= ?2 \
             ORDER BY started_at DESC, id DESC LIMIT 1",
            params![slug, scheduled_for],
        )
        .await
        .context("query successful scheduled-slot coverage")?;
    one_run(rows, "successful scheduled-slot coverage").await
}

async fn one_run(mut rows: Rows, label: &'static str) -> OperationResult<Option<SyncRun>> {
    let Some(row) = rows.next().await.with_context(|| format!("read {label}"))? else {
        return Ok(None);
    };
    Ok(Some(sync_run_from_row(&row)?))
}

async fn collect_runs(mut rows: Rows, label: &'static str) -> OperationResult<Vec<SyncRun>> {
    let mut runs = Vec::new();
    while let Some(row) = rows.next().await.with_context(|| format!("read {label}"))? {
        runs.push(sync_run_from_row(&row)?);
    }
    Ok(runs)
}

fn control_from_row(row: &Row) -> OperationResult<OperationControl> {
    let mode = row
        .get::<String>(0)
        .context("decode operation mode")?
        .parse()?;
    let scheduler_enabled = row.get::<i64>(1).context("decode scheduler-enabled flag")?;
    let scheduler_enabled = match scheduler_enabled {
        0 => false,
        1 => true,
        value => {
            return Err(OperationError::coordination(format!(
                "database contains unsupported scheduler-enabled value {value}"
            )));
        }
    };
    Ok(OperationControl {
        mode,
        scheduler_enabled,
        updated_at: row.get(2).context("decode operation-control update time")?,
    })
}

fn sync_run_from_row(row: &Row) -> OperationResult<SyncRun> {
    let status = SyncRunStatus::from_str(&row.get::<String>(5).context("decode sync-run status")?)?;
    Ok(SyncRun {
        id: row.get(0).context("decode sync-run id")?,
        connection_slug: row.get(1).context("decode sync-run connection")?,
        scheduled_for: row.get(2).context("decode sync-run scheduled slot")?,
        started_at: row.get(3).context("decode sync-run start time")?,
        finished_at: row.get(4).context("decode sync-run finish time")?,
        status,
        error_message: row.get(6).context("decode sync-run error")?,
        endpoints_hit: row.get(7).context("decode sync-run endpoints")?,
        events_upserted: row.get(8).context("decode sync-run event count")?,
        loans_upserted: row.get(9).context("decode sync-run loan count")?,
        snapshots_created: row.get(10).context("decode sync-run snapshot count")?,
    })
}

fn validate_completion(completion: &SyncCompletion) -> OperationResult<()> {
    for (label, value) in [
        ("events upserted", completion.events_upserted),
        ("loans upserted", completion.loans_upserted),
        ("snapshots created", completion.snapshots_created),
    ] {
        if value < 0 {
            return Err(OperationError::validation(format!(
                "{label} cannot be negative"
            )));
        }
    }
    Ok(())
}

fn require_nonempty(label: &str, value: &str) -> OperationResult<()> {
    if value.trim().is_empty() {
        return Err(OperationError::validation(format!(
            "{label} cannot be empty"
        )));
    }
    Ok(())
}

fn require_timestamp(label: &str, value: &str) -> OperationResult<()> {
    let bytes = value.as_bytes();
    let delimiters_are_canonical = bytes.len() == 24
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'.'
        && bytes[23] == b'Z';
    let numeric_positions_are_digits = bytes.len() == 24
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19 | 23) || byte.is_ascii_digit()
        });
    if !delimiters_are_canonical || !numeric_positions_are_digits {
        return Err(OperationError::validation(format!(
            "{label} must use fixed-width UTC RFC3339 milliseconds (YYYY-MM-DDTHH:MM:SS.mmmZ)"
        )));
    }
    Ok(())
}

fn require_positive_id(label: &str, value: i64) -> OperationResult<()> {
    if value <= 0 {
        return Err(OperationError::validation(format!(
            "{label} id must be positive"
        )));
    }
    Ok(())
}

const fn bool_i64(value: bool) -> i64 {
    if value { 1 } else { 0 }
}
