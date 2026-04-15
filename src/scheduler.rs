use std::str::FromStr;
use std::sync::Arc;

use chrono::Utc;
use cron::Schedule;
use sqlx::PgPool;

use crate::AppState;

/// Convert a standard 5-field cron expression (min hour day month weekday)
/// into the 7-field format the `cron` crate expects (sec min hour day month weekday year).
fn to_seven_field(expr: &str) -> String {
    format!("0 {} *", expr)
}

/// Background loop that checks integration cron schedules and spawns syncs when due.
///
/// Runs forever — call via `tokio::spawn` at startup.
pub async fn run(state: Arc<AppState>) {
    tracing::info!("scheduler: started");

    loop {
        let next_sleep = match tick(&state.db).await {
            Ok(dur) => dur,
            Err(e) => {
                tracing::error!("scheduler: tick error: {e}");
                tokio::time::Duration::from_secs(60)
            }
        };

        tracing::debug!("scheduler: sleeping {}s until next check", next_sleep.as_secs());
        tokio::time::sleep(next_sleep).await;
    }
}

/// One tick: check all scheduled connections, spawn syncs for any that are due,
/// and return how long to sleep until the next earliest fire time.
async fn tick(pool: &PgPool) -> anyhow::Result<tokio::time::Duration> {
    let connections = crate::db::integrations::list_scheduled_connections(pool).await;

    if connections.is_empty() {
        // Nothing scheduled — check again in 5 minutes in case someone adds one.
        return Ok(tokio::time::Duration::from_secs(300));
    }

    let now = Utc::now();
    let mut earliest_next = None;

    for (slug, cadence) in &connections {
        let seven = to_seven_field(cadence);
        let schedule = match Schedule::from_str(&seven) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("scheduler: invalid cron '{cadence}' for {slug}: {e}");
                continue;
            }
        };

        // Find the next fire time from now.
        let Some(next) = schedule.upcoming(Utc).next() else {
            continue;
        };

        // Check if we should fire: if the next fire time is within 30 seconds of now,
        // or if the *previous* fire time was missed (within the last tick window).
        let prev = schedule.after(&(now - chrono::Duration::seconds(61))).next();
        let should_fire = prev.is_some_and(|p| p <= now && (now - p).num_seconds() < 60);

        if should_fire {
            tracing::info!("scheduler: triggering sync for '{slug}'");
            let pool_clone = pool.clone();
            let slug_clone = slug.clone();
            tokio::spawn(async move {
                if let Err(e) = run_scheduled_sync(&pool_clone, &slug_clone).await {
                    tracing::error!("scheduler: sync failed for '{slug_clone}': {e}");
                }
            });
        }

        // Track the soonest next fire across all connections.
        match earliest_next {
            None => earliest_next = Some(next),
            Some(ref existing) if next < *existing => earliest_next = Some(next),
            _ => {}
        }
    }

    // Sleep until the earliest next fire, with a 5-second buffer so we don't wake up
    // a fraction of a second too early. Cap at 5 minutes to pick up new schedules.
    let sleep_secs = earliest_next
        .map(|next| {
            let delta = (next - now).num_seconds().max(5);
            (delta as u64).min(300)
        })
        .unwrap_or(300);

    Ok(tokio::time::Duration::from_secs(sleep_secs))
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
