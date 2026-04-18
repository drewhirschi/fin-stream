use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::get};
use serde_json::json;
use std::sync::Arc;

use crate::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/healthz", get(|| async { "ok" }))
        .route("/ready", get(readiness_check))
}

/// `/ready` — readiness probe that verifies DB connectivity.
/// Returns 200 when the app can serve traffic, 503 when the DB is
/// unreachable.  Used by Coolify as the health check for zero-downtime
/// deploys: the new container only receives traffic once this returns 200.
async fn readiness_check(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
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

/// Router for health endpoints that require auth (i.e. anything that reveals
/// config detail, even indirectly). Mounted under the protected middleware
/// layer in `main.rs`.
pub fn protected_router() -> Router<Arc<AppState>> {
    Router::new().route("/health/crypto", get(crypto_selftest))
}

/// `/health/crypto` — round-trip encrypt → decrypt a known plaintext so
/// operators can validate that the configured APP_ENCRYPTION_KEY is
/// functional immediately after deploy. Returns the key fingerprint (first
/// 8 hex chars of SHA-256(key)) for diffing across deploys. Never leaks the
/// raw key.
async fn crypto_selftest() -> impl IntoResponse {
    let probe = "trust-deeds-crypto-selftest";
    let result = crate::crypto::encrypt_string(probe)
        .and_then(|(ct, nonce)| crate::crypto::decrypt_string(&ct, &nonce));

    match result {
        Ok(decoded) if decoded == probe => {
            let fingerprint = crate::config::app_encryption_key_fingerprint();
            (
                StatusCode::OK,
                Json(json!({
                    "status": "ok",
                    "key_fingerprint": fingerprint,
                })),
            )
                .into_response()
        }
        Ok(other) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "status": "error",
                "reason": "round_trip_mismatch",
                "got_len": other.len(),
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "status": "error",
                "reason": "round_trip_failed",
                "detail": e.to_string(),
            })),
        )
            .into_response(),
    }
}
