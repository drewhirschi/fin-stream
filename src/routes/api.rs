use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, patch, post},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;
use crate::db;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/forecast", get(get_forecast))
        .route("/api/events", post(create_event))
        .route("/api/events/{id}", patch(update_event))
        .route("/api/settings/cash", post(set_cash_balance))
        .route("/api/sync/balance", post(sync_monarch_balance))
}

#[derive(Deserialize)]
struct ForecastQuery {
    from: Option<String>,
    through: Option<String>,
    stream_id: Option<i64>,
}

async fn get_forecast(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ForecastQuery>,
) -> impl IntoResponse {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let from = params.from.unwrap_or_else(|| today.clone());
    let through = params.through.unwrap_or_else(|| {
        (chrono::Utc::now() + chrono::Duration::days(180))
            .format("%Y-%m-%d")
            .to_string()
    });

    // Validate date formats
    if chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d").is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "bad_request", "message": "Invalid 'from' date format. Use YYYY-MM-DD."})),
        ).into_response();
    }
    if chrono::NaiveDate::parse_from_str(&through, "%Y-%m-%d").is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "bad_request", "message": "Invalid 'through' date format. Use YYYY-MM-DD."})),
        ).into_response();
    }

    match db::forecasts::compute_forecast(&state.db, &from, &through, params.stream_id).await {
        Ok(Some(forecast)) => Json(serde_json::to_value(forecast).unwrap()).into_response(),
        Ok(None) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "error": "no_starting_balance",
                "message": "Set your current cash balance in Settings."
            })),
        ).into_response(),
        Err(e) => {
            tracing::error!("forecast computation failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal", "message": "Forecast computation failed."})),
            ).into_response()
        }
    }
}

#[derive(Deserialize)]
struct CreateEventRequest {
    stream_id: i64,
    label: String,
    scheduled_date: String,
    amount: f64,
}

async fn create_event(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateEventRequest>,
) -> impl IntoResponse {
    if req.amount == 0.0 {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "validation_error", "message": "Amount cannot be zero."})),
        ).into_response();
    }

    if req.label.trim().is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "validation_error", "message": "Label is required."})),
        ).into_response();
    }

    if chrono::NaiveDate::parse_from_str(&req.scheduled_date, "%Y-%m-%d").is_err() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "validation_error", "message": "Invalid date format. Use YYYY-MM-DD."})),
        ).into_response();
    }

    // Verify stream exists
    let stream_exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM stream WHERE id = ?")
        .bind(req.stream_id)
        .fetch_optional(&state.db)
        .await
        .unwrap_or(None);

    if stream_exists.is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "stream_not_found", "message": "Stream does not exist."})),
        ).into_response();
    }

    match db::events::create_event(
        &state.db,
        req.stream_id,
        &req.label,
        &req.scheduled_date,
        req.amount,
        "projected",
        "manual",
        None,
    )
    .await
    {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response(),
        Err(e) => {
            tracing::error!("failed to create event: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal", "message": "Failed to create event."})),
            ).into_response()
        }
    }
}

#[derive(Deserialize)]
struct UpdateEventRequest {
    expected_date: String,
}

async fn update_event(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateEventRequest>,
) -> impl IntoResponse {
    if chrono::NaiveDate::parse_from_str(&req.expected_date, "%Y-%m-%d").is_err() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "validation_error", "message": "Invalid date format. Use YYYY-MM-DD."})),
        ).into_response();
    }

    // Check if event exists and is not received
    let event: Option<(String,)> =
        sqlx::query_as("SELECT status FROM stream_event WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.db)
            .await
            .unwrap_or(None);

    match event {
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "not_found", "message": "Event not found."})),
        ).into_response(),
        Some((status,)) if status == "received" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "cannot_override_received", "message": "Cannot override received payments."})),
        ).into_response(),
        Some(_) => {
            match db::events::override_event_date(&state.db, id, &req.expected_date).await {
                Ok(true) => Json(serde_json::json!({"ok": true})).into_response(),
                Ok(false) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "internal", "message": "Update failed."})),
                ).into_response(),
                Err(e) => {
                    tracing::error!("failed to update event {id}: {e}");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": "internal", "message": "Update failed."})),
                    ).into_response()
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct SetCashRequest {
    amount: f64,
}

async fn set_cash_balance(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SetCashRequest>,
) -> impl IntoResponse {
    match db::forecasts::set_starting_balance(&state.db, req.amount).await {
        Ok(()) => Json(serde_json::json!({"ok": true})),
        Err(e) => {
            tracing::error!("failed to set cash balance: {e}");
            Json(serde_json::json!({"error": "internal", "message": "Failed to set balance."}))
        }
    }
}

/// POST /api/sync/balance — pull current balance from Monarch and store as starting cash.
async fn sync_monarch_balance(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let client = match crate::monarch::client::create_client() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("failed to create Monarch client: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "config", "message": "MONARCH_TOKEN not set. Add it to .env."})),
            ).into_response();
        }
    };

    let account_id = crate::config::monarch_account_id();

    match client.get_adjusted_balance(&account_id).await {
        Ok((balance, adjusted, pending_total)) => {
            // Use the adjusted balance (reported minus pending) as the starting cash
            match db::forecasts::set_starting_balance(&state.db, adjusted).await {
                Ok(()) => {
                    let _ = sqlx::query(
                        "INSERT INTO settings (key, value) VALUES ('balance_source', ?)
                         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                    )
                    .bind(format!(
                        "monarch:{}:reported={:.2}:pending={:.2}:adjusted={:.2}:{}",
                        balance.display_name,
                        balance.current_balance,
                        pending_total,
                        adjusted,
                        balance.updated_at
                    ))
                    .execute(&state.db)
                    .await;

                    Json(serde_json::json!({
                        "ok": true,
                        "reported_balance": balance.current_balance,
                        "pending_total": pending_total,
                        "adjusted_balance": adjusted,
                        "account": balance.display_name,
                        "updated_at": balance.updated_at,
                    }))
                    .into_response()
                }
                Err(e) => {
                    tracing::error!("failed to save Monarch balance: {e}");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": "internal", "message": "Failed to save balance."})),
                    ).into_response()
                }
            }
        }
        Err(e) => {
            tracing::error!("Monarch balance fetch failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": "monarch_error",
                    "message": format!("Failed to fetch balance from Monarch: {e}")
                })),
            ).into_response()
        }
    }
}
