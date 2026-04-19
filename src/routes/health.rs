use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::get};
use serde_json::json;
use std::sync::Arc;

use crate::AppState;
use crate::models::*;
use crate::templates;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/healthz", get(|| async { "ok" }))
        .route("/ready", get(readiness_check))
        .route("/bench/render", get(bench_render))
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

/// `/bench/render` — renders the integration overview template with hardcoded
/// dummy data. No DB, no auth. Measures pure template rendering throughput.
async fn bench_render() -> impl IntoResponse {
    let loans: Vec<LoanView> = (0..15)
        .map(|i| LoanView {
            loan_account: format!("LOAN-{i:04}"),
            borrower_name: Some(format!("Borrower {i}")),
            property_address: Some(format!("{} Oak Street", 100 + i)),
            property_city: Some("Salt Lake City".into()),
            property_state: Some("UT".into()),
            featured_image_url: None,
            property_type: Some("Single Family".into()),
            percent_owned: Some(100.0),
            note_rate: Some(8.5 + (i as f64) * 0.25),
            principal_balance: Some(150_000.0 + (i as f64) * 10_000.0),
            regular_payment: Some(1_200.0 + (i as f64) * 50.0),
            maturity_date: Some("2029-06-01".into()),
            next_payment_date: Some("2026-05-01".into()),
            interest_paid_to: Some("2026-04-01".into()),
            is_delinquent: Some(0),
        })
        .collect();

    let payments: Vec<PaymentView> = (0..8)
        .map(|i| PaymentView {
            id: i + 1,
            label: Some(format!("Payment from Borrower {i}")),
            scheduled_date: format!("2026-04-{:02}", 1 + i),
            expected_date: None,
            actual_date: Some(format!("2026-04-{:02}", 1 + i)),
            amount: 1_200.0 + (i as f64) * 50.0,
            status: "received".into(),
            source_type: Some("tmo_history".into()),
            is_pending_print_check: false,
            check_number: None,
            loan_account: Some(format!("LOAN-{i:04}")),
            metadata: None,
        })
        .collect();

    let connection = IntegrationConnectionView {
        id: 1,
        slug: "tmo".into(),
        name: "The Mortgage Office".into(),
        provider: "tmo".into(),
        status: "active".into(),
        sync_cadence: "every_6h".into(),
        last_synced_at: Some("2026-04-18T12:00:00Z".into()),
        last_error: None,
        next_scheduled_at: Some("2026-04-18T18:00:00Z".into()),
        record_count: 15,
        normalized_count: 120,
        pending_count: 0,
    };

    templates::IntegrationOverviewTemplate {
        title: "Trust Deeds - The Mortgage Office".into(),
        current_section: "overview".into(),
        loans,
        payments,
        portfolio_value: Some(2_450_000.0),
        portfolio_yield: Some(9.2),
        ytd_interest: Some(85_000.0),
        trust_balance: Some(125_000.0),
        outstanding_checks: Some(3_500.0),
        active_loans_count: 15,
        connection,
    }
    .into_response()
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
