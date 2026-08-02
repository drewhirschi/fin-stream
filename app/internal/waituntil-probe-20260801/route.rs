use axum::{Json, response::{IntoResponse, Response}};
use serde_json::json;
use std::time::{Duration, Instant};

/// TEMPORARY diagnostic probe (2026-08-01): demonstrates that `wait_until`
/// background futures stop executing once the response is sent and only
/// advance during later invocations. Registers a 30-tick, once-per-second
/// logger; each tick records wall time and monotonic elapsed time so
/// suspension gaps are visible in the runtime logs. Spawns log-only work —
/// no state is read or written. Remove with its `is_public` exemption once
/// the Vercel issue is filed.
pub async fn get(wait: nextrs::WaitUntil) -> Response {
    let started_wall = time::OffsetDateTime::now_utc();
    let started = Instant::now();
    tracing::info!(started_wall = %started_wall, "waituntil-probe scheduled");
    wait.wait_until(async move {
        for tick in 1..=30_u32 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            tracing::info!(
                tick,
                elapsed_ms = started.elapsed().as_millis() as u64,
                wall = %time::OffsetDateTime::now_utc(),
                "waituntil-probe tick"
            );
        }
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            "waituntil-probe DONE"
        );
    });
    Json(json!({
        "scheduled": true,
        "ticks": 30,
        "started_wall": started_wall.to_string(),
    }))
    .into_response()
}
