use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Duration, Timelike, Utc};
use cron::Schedule;
use sqlx::PgPool;

use crate::AppState;

/// Typed sync cadence. Persisted in `intg.integration_connection.sync_cadence`
/// as one of the string values below. This replaces the previous free-form
/// cron expression column to avoid "0 21 * * *" style typos sending a 4×/day
/// intent to once-daily.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncCadence {
    /// Every hour at :00.
    Hourly,
    /// 00:00, 06:00, 12:00, 18:00 UTC.
    Every6h,
    /// 00:00, 12:00 UTC.
    Every12h,
    /// Once per day at 06:00 UTC.
    Daily,
    /// No automatic sync.
    Manual,
}

impl SyncCadence {
    /// Default cadence for the TMO integration when no row exists yet.
    pub const fn default_for_tmo() -> Self {
        SyncCadence::Every6h
    }

    /// Canonical lowercase string persisted in the DB and sent by the UI.
    pub fn as_str(&self) -> &'static str {
        match self {
            SyncCadence::Hourly => "hourly",
            SyncCadence::Every6h => "every_6h",
            SyncCadence::Every12h => "every_12h",
            SyncCadence::Daily => "daily",
            SyncCadence::Manual => "manual",
        }
    }

    /// Parse a stored cadence value. Accepts the canonical enum strings, a few
    /// common aliases, and legacy 5-field cron expressions (which we map to
    /// the closest enum value so migration is automatic — we never execute
    /// arbitrary cron anymore).
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Some(SyncCadence::Manual);
        }
        let lower = trimmed.to_ascii_lowercase();
        match lower.as_str() {
            "manual" | "off" | "disabled" => Some(SyncCadence::Manual),
            "hourly" | "every_hour" | "1h" => Some(SyncCadence::Hourly),
            "every_6h" | "6h" | "4x_daily" | "every_6_hours" => Some(SyncCadence::Every6h),
            "every_12h" | "12h" | "2x_daily" | "every_12_hours" => Some(SyncCadence::Every12h),
            "daily" | "1x_daily" | "24h" | "every_day" => Some(SyncCadence::Daily),
            _ => Self::from_legacy_cron(&lower),
        }
    }

    /// Best-effort mapping from a 5-field cron expression to one of our
    /// supported cadences. Used only on read to migrate rows written before
    /// the enum was introduced. Unknown patterns fall back to `Daily` rather
    /// than `Manual` because a row with *any* cron was clearly meant to run.
    fn from_legacy_cron(expr: &str) -> Option<Self> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return None;
        }
        let (_min, hour, dom, month, dow) = (fields[0], fields[1], fields[2], fields[3], fields[4]);
        // Only accept "every day" shapes.
        if dom != "*" || month != "*" || dow != "*" {
            return Some(SyncCadence::Daily);
        }
        match hour {
            "*" => Some(SyncCadence::Hourly),
            "*/6" | "0,6,12,18" => Some(SyncCadence::Every6h),
            "*/12" | "0,12" => Some(SyncCadence::Every12h),
            _ => Some(SyncCadence::Daily),
        }
    }

    /// Underlying 7-field cron expression used to compute fire times. For
    /// `Manual` there is no schedule.
    fn cron(&self) -> Option<&'static str> {
        match self {
            SyncCadence::Hourly => Some("0 0 * * * * *"),
            SyncCadence::Every6h => Some("0 0 0,6,12,18 * * * *"),
            SyncCadence::Every12h => Some("0 0 0,12 * * * *"),
            SyncCadence::Daily => Some("0 0 6 * * * *"),
            SyncCadence::Manual => None,
        }
    }

    fn schedule(&self) -> Option<Schedule> {
        self.cron().and_then(|expr| Schedule::from_str(expr).ok())
    }

    /// The most recent scheduled fire time at or before `now`. Used to detect
    /// a missed run that happened while the process was down.
    pub fn previous_fire(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let schedule = self.schedule()?;
        // `cron::Schedule` doesn't offer a direct "previous" iterator, so we
        // walk forward from a point ~2 hours before `now` and take the last
        // fire time that's <= now. Our cadences all fire at least every 24h,
        // but we back off further just to be safe.
        let start = now - Duration::hours(25);
        let mut last = None;
        for fire in schedule.after(&start).take(200) {
            if fire > now {
                break;
            }
            last = Some(fire);
        }
        last
    }

    /// The next fire time strictly after `now`.
    pub fn next_fire(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.schedule()?.after(&now).next()
    }
}

/// How far back on startup we look for a missed scheduled slot and re-fire it.
const MISSED_RUN_WINDOW_HOURS: i64 = 2;

/// Background loop that checks integration cadences and spawns syncs when due.
/// Runs forever — call via `tokio::spawn` at startup.
pub async fn run(state: Arc<AppState>) {
    tracing::info!("scheduler: started");

    // One-time upgrade: if the TMO connection exists with the legacy 'manual'
    // cadence, bump it to every_6h. No-op otherwise.
    if let Err(e) = crate::db::integrations::ensure_tmo_default_cadence(&state.db).await {
        tracing::warn!("scheduler: failed to ensure TMO default cadence: {e}");
    }

    // On boot, look for scheduled connections whose most recent cron fire
    // happened within the last `MISSED_RUN_WINDOW_HOURS` *and* that hasn't
    // already been covered by a later successful sync_log. Fire those
    // immediately before entering the steady-state tick loop.
    if let Err(e) = backfill_missed_runs(&state.db).await {
        tracing::error!("scheduler: backfill failed: {e}");
    }

    loop {
        let next_sleep = match tick(&state).await {
            Ok(dur) => dur,
            Err(e) => {
                tracing::error!("scheduler: tick error: {e}");
                tokio::time::Duration::from_secs(60)
            }
        };

        tracing::debug!(
            "scheduler: sleeping {}s until next check",
            next_sleep.as_secs()
        );
        tokio::time::sleep(next_sleep).await;
    }
}

async fn backfill_missed_runs(pool: &PgPool) -> anyhow::Result<()> {
    let now = Utc::now();
    let connections = crate::db::integrations::list_scheduled_connections(pool).await;

    for (slug, raw_cadence) in connections {
        let Some(cadence) = SyncCadence::parse(&raw_cadence) else {
            tracing::warn!(
                "scheduler: unknown cadence '{raw_cadence}' for '{slug}' — skipping backfill"
            );
            continue;
        };
        if matches!(cadence, SyncCadence::Manual) {
            continue;
        }
        let Some(prev) = cadence.previous_fire(now) else {
            continue;
        };

        // Only consider slots missed within the recent window — we don't want
        // to fire syncs for weeks-old gaps.
        if (now - prev).num_hours() > MISSED_RUN_WINDOW_HOURS {
            continue;
        }

        let last_success = crate::db::integrations::last_successful_sync_started_at(pool, &slug)
            .await;

        let missed = match last_success {
            Some(ts) => ts < prev,
            None => true,
        };

        if missed {
            tracing::info!(
                "scheduler: backfilling missed run for '{slug}' (prev fire {prev}, last success \
                 {last_success:?})"
            );
            let pool_clone = pool.clone();
            let slug_clone = slug.clone();
            tokio::spawn(async move {
                if let Err(e) = run_scheduled_sync(&pool_clone, &slug_clone).await {
                    tracing::error!("scheduler: backfill sync failed for '{slug_clone}': {e}");
                }
            });
        }
    }
    Ok(())
}

/// One tick: check all scheduled connections, spawn syncs for any that are
/// due, persist `next_scheduled_at` for observability, and return how long to
/// sleep until the earliest next fire time.
async fn tick(state: &Arc<AppState>) -> anyhow::Result<tokio::time::Duration> {
    let pool = &state.db;
    let connections = crate::db::integrations::list_scheduled_connections(pool).await;

    if connections.is_empty() {
        // Nothing scheduled — check again in 5 minutes in case someone adds one.
        return Ok(tokio::time::Duration::from_secs(300));
    }

    let now = Utc::now();
    let mut earliest_next = None;

    for (slug, raw_cadence) in &connections {
        let Some(cadence) = SyncCadence::parse(raw_cadence) else {
            tracing::warn!("scheduler: invalid cadence '{raw_cadence}' for {slug}");
            continue;
        };
        if matches!(cadence, SyncCadence::Manual) {
            continue;
        }

        let Some(next) = cadence.next_fire(now) else {
            continue;
        };

        // Fire if the previous scheduled slot was within the last 60s — that
        // means we woke up right on it.
        let should_fire = cadence
            .previous_fire(now)
            .is_some_and(|prev| (now - prev).num_seconds() < 60);

        // Persist the next fire time for UI observability. Failures here are
        // not fatal — it's just a display column.
        let next_iso = next.to_rfc3339();
        if let Err(e) = crate::db::integrations::update_connection_next_scheduled_at(
            pool,
            slug,
            Some(&next_iso),
        )
        .await
        {
            tracing::debug!("scheduler: failed to persist next_scheduled_at for '{slug}': {e}");
        }

        if should_fire {
            tracing::info!("scheduler: triggering sync for '{slug}'");
            let state_clone = Arc::clone(state);
            let slug_clone = slug.clone();
            tokio::spawn(async move {
                if let Err(e) = run_scheduled_sync(&state_clone.db, &slug_clone).await {
                    tracing::error!("scheduler: sync failed for '{slug_clone}': {e}");
                }
                state_clone.page_cache.invalidate_all().await;
            });
        }

        match earliest_next {
            None => earliest_next = Some(next),
            Some(ref existing) if next < *existing => earliest_next = Some(next),
            _ => {}
        }
    }

    // Sleep until the earliest next fire, with a small safety margin. Cap at
    // 5 minutes so newly-added schedules get picked up in reasonable time.
    let sleep_secs = earliest_next
        .map(|next| {
            let delta = (next - now).num_seconds().max(5);
            (delta as u64).min(300)
        })
        .unwrap_or(300);

    // Align to the minute when the next fire is close, so we don't wake up a
    // fraction of a second late and miss the 60s window.
    let aligned = if sleep_secs < 120 {
        let now_secs = now.second() as u64;
        if now_secs == 0 { sleep_secs } else { sleep_secs + 1 }
    } else {
        sleep_secs
    };

    Ok(tokio::time::Duration::from_secs(aligned))
}

async fn run_scheduled_sync(pool: &PgPool, slug: &str) -> anyhow::Result<()> {
    match slug {
        "tmo" => {
            let summary = crate::tmo::sync::run_full_sync(pool).await?;
            tracing::info!(
                "scheduler: tmo sync complete — {} loans, {} payments",
                summary.loans_upserted,
                summary.events_upserted
            );
        }
        other => {
            tracing::warn!("scheduler: no sync implementation for '{other}'");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_canonical() {
        assert_eq!(SyncCadence::parse("hourly"), Some(SyncCadence::Hourly));
        assert_eq!(SyncCadence::parse("every_6h"), Some(SyncCadence::Every6h));
        assert_eq!(SyncCadence::parse("every_12h"), Some(SyncCadence::Every12h));
        assert_eq!(SyncCadence::parse("daily"), Some(SyncCadence::Daily));
        assert_eq!(SyncCadence::parse("manual"), Some(SyncCadence::Manual));
        assert_eq!(SyncCadence::parse(""), Some(SyncCadence::Manual));
    }

    #[test]
    fn parse_legacy_cron_daily() {
        // "0 21 * * *" — once daily at 9pm UTC, the pre-fix TMO value.
        assert_eq!(SyncCadence::parse("0 21 * * *"), Some(SyncCadence::Daily));
    }

    #[test]
    fn parse_legacy_cron_hourly() {
        assert_eq!(SyncCadence::parse("0 * * * *"), Some(SyncCadence::Hourly));
    }

    #[test]
    fn parse_legacy_cron_every_6h() {
        assert_eq!(
            SyncCadence::parse("0 */6 * * *"),
            Some(SyncCadence::Every6h)
        );
        assert_eq!(
            SyncCadence::parse("0 0,6,12,18 * * *"),
            Some(SyncCadence::Every6h)
        );
    }

    #[test]
    fn next_fire_is_in_future() {
        let now = Utc::now();
        let next = SyncCadence::Every6h.next_fire(now).unwrap();
        assert!(next > now);
    }

    #[test]
    fn previous_fire_is_in_past() {
        let now = Utc::now();
        let prev = SyncCadence::Every6h.previous_fire(now).unwrap();
        assert!(prev <= now);
        assert!((now - prev).num_hours() < 7);
    }
}
