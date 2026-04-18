# Readiness Endpoint (`/ready`)

## Goal
Add a `/ready` endpoint that verifies the app can serve traffic by checking DB connectivity. This complements the existing `/health` (liveness) endpoint.

## Current state
- `/health` and `/healthz` return `"ok"` unconditionally (liveness probes)
- No readiness check exists

## Implementation

### Add `/ready` to `src/routes/health.rs`

Add a new route to the public `router()` function:

```rust
.route("/ready", get(readiness_check))
```

The handler:

```rust
async fn readiness_check(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
    {
        Ok(_) => (StatusCode::OK, Json(json!({"status": "ok"}))).into_response(),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "unavailable",
                "reason": "db_unreachable",
                "detail": e.to_string(),
            })),
        )
            .into_response(),
    }
}
```

### Key decisions
- Returns **503 Service Unavailable** when DB is down (standard for readiness probes)
- Uses `SELECT 1` — cheapest possible DB round-trip
- Public (no auth required) so Coolify/load balancers can poll it
- Returns JSON for consistency with `/health/crypto`

### Load testing
Once implemented, load test `/health` and `/ready` together with `oha` to verify the ready endpoint doesn't fall over under load (it does a DB call per request, so connection pool pressure is the thing to watch).
