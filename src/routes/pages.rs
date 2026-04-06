use axum::{Router, extract::State, routing::get};
use std::sync::Arc;

use crate::AppState;
use crate::db;
use crate::templates;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(index))
        .route("/loans", get(loans))
        .route("/payments", get(payments))
        .route("/forecast", get(forecast))
}

async fn index(State(state): State<Arc<AppState>>) -> templates::IndexTemplate {
    let loans = db::loans::get_active_loans(&state.db).await;
    let recent_payments = db::events::get_recent_payments(&state.db, 10).await;
    let upcoming = db::events::get_upcoming_payments(&state.db, 10).await;

    let snapshot: Option<(Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT portfolio_value, portfolio_yield, ytd_interest, trust_balance, outstanding_checks
         FROM portfolio_snapshot ORDER BY snapshot_date DESC LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);

    let (portfolio_value, portfolio_yield, ytd_interest, trust_balance, outstanding_checks) = snapshot
        .unwrap_or((None, None, None, None, None));

    templates::IndexTemplate {
        title: "Trust Deeds - Dashboard".into(),
        loans,
        recent_payments,
        upcoming,
        portfolio_value,
        portfolio_yield,
        ytd_interest,
        trust_balance,
        outstanding_checks,
    }
}

async fn loans(State(state): State<Arc<AppState>>) -> templates::LoansTemplate {
    let loans = db::loans::get_active_loans(&state.db).await;

    templates::LoansTemplate {
        title: "Trust Deeds - Loans".into(),
        loans,
    }
}

async fn payments(State(state): State<Arc<AppState>>) -> templates::PaymentsTemplate {
    let payments = db::events::get_all_payments(&state.db, 50).await;

    templates::PaymentsTemplate {
        title: "Trust Deeds - Payments".into(),
        payments,
    }
}

async fn forecast(State(state): State<Arc<AppState>>) -> templates::ForecastTemplate {
    let has_balance = db::forecasts::get_starting_balance(&state.db).await.is_some();

    // Ensure expenses stream exists for the outflow form
    let expenses_stream_id = db::ensure_expenses_stream(&state.db).await.unwrap_or(2);

    let streams: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, name FROM stream WHERE is_active = 1 ORDER BY name",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    templates::ForecastTemplate {
        title: "Trust Deeds - Forecast".into(),
        has_balance,
        streams,
        expenses_stream_id,
    }
}
