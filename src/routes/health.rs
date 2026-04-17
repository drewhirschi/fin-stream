use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::get};
use serde_json::json;
use std::sync::Arc;

use crate::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/healthz", get(|| async { "ok" }))
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
