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

/// Invalidate all page cache zones affected by event/stream/account mutations.
async fn invalidate_data_caches(state: &AppState) {
    state.page_cache.invalidate("forecast").await;
    state.page_cache.invalidate("streams").await;
    state.page_cache.invalidate_prefix("tmo:").await;
    state.page_cache.invalidate("integrations").await;
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/forecast", get(get_forecast))
        .route("/api/events", post(create_event))
        .route("/api/events/{id}", patch(update_event))
        .route("/api/accounts", post(create_account))
        .route("/api/accounts/{id}", patch(update_account))
        .route("/api/streams", post(create_stream))
        .route("/api/streams/{id}", patch(update_stream))
        .route("/api/views", post(create_view))
        .route("/api/views/{id}", patch(update_view))
        .route("/api/settings/cash", post(set_cash_balance))
        .route("/api/sync/balance", post(sync_monarch_balance))
}

#[derive(Deserialize)]
struct ForecastQuery {
    from: Option<String>,
    through: Option<String>,
    stream_id: Option<i64>,
    view_id: Option<i64>,
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

    if let Some(view_id) = params.view_id {
        match db::streams::view_exists(&state.db, view_id).await {
            Ok(true) => {}
            Ok(false) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "view_not_found", "message": "View does not exist."})),
                )
                    .into_response();
            }
            Err(e) => {
                tracing::error!("view existence check failed: {e}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "internal", "message": "Failed to load forecast."})),
                )
                    .into_response();
            }
        }
    }

    match db::forecasts::compute_forecast(
        &state.db,
        &from,
        &through,
        params.stream_id,
        params.view_id,
    )
    .await
    {
        Ok(Some(forecast)) => Json(serde_json::to_value(forecast).unwrap()).into_response(),
        Ok(None) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "error": "no_starting_balance",
                "message": "Set your current cash balance in Settings."
            })),
        )
            .into_response(),
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
    account_id: Option<i64>,
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
        )
            .into_response();
    }

    if chrono::NaiveDate::parse_from_str(&req.scheduled_date, "%Y-%m-%d").is_err() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "validation_error", "message": "Invalid date format. Use YYYY-MM-DD."})),
        ).into_response();
    }

    match db::streams::stream_exists(&state.db, req.stream_id).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "stream_not_found", "message": "Stream does not exist."})),
            ).into_response();
        }
        Err(e) => {
            tracing::error!("stream existence check failed: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    serde_json::json!({"error": "internal", "message": "Failed to create event."}),
                ),
            )
                .into_response();
        }
    }

    if let Some(account_id) = req.account_id {
        match db::accounts::account_exists(&state.db, account_id).await {
            Ok(true) => {}
            Ok(false) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "account_not_found", "message": "Account does not exist."})),
                )
                    .into_response();
            }
            Err(e) => {
                tracing::error!("account existence check failed: {e}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "internal", "message": "Failed to create event."})),
                )
                    .into_response();
            }
        }
    }

    match db::events::create_event(
        &state.db,
        req.stream_id,
        req.account_id,
        &req.label,
        &req.scheduled_date,
        req.amount,
        "projected",
        "manual",
        None,
    )
    .await
    {
        Ok(id) => {
            invalidate_data_caches(&state).await;
            (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response()
        }
        Err(e) => {
            tracing::error!("failed to create event: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    serde_json::json!({"error": "internal", "message": "Failed to create event."}),
                ),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct UpdateEventRequest {
    label: Option<String>,
    amount: Option<f64>,
    expected_date: Option<String>,
    account_id: Option<i64>,
}

async fn update_event(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateEventRequest>,
) -> impl IntoResponse {
    if let Some(expected_date) = req.expected_date.as_deref() {
        if chrono::NaiveDate::parse_from_str(expected_date, "%Y-%m-%d").is_err() {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": "validation_error", "message": "Invalid date format. Use YYYY-MM-DD."})),
            ).into_response();
        }
    }

    if let Some(amount) = req.amount {
        if amount == 0.0 {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": "validation_error", "message": "Amount cannot be zero."})),
            )
                .into_response();
        }
    }

    if let Some(account_id) = req.account_id {
        match db::accounts::account_exists(&state.db, account_id).await {
            Ok(true) => {}
            Ok(false) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "account_not_found", "message": "Account does not exist."})),
                )
                    .into_response();
            }
            Err(e) => {
                tracing::error!("account existence check failed: {e}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "internal", "message": "Update failed."})),
                )
                    .into_response();
            }
        }
    }

    let event: Option<(String,)> = sqlx::query_as("SELECT status FROM stream_event WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .unwrap_or(None);

    match event {
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "not_found", "message": "Event not found."})),
        )
            .into_response(),
        Some((status,)) if status == "received" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "cannot_override_received", "message": "Cannot edit received payments."})),
        )
            .into_response(),
        Some(_) => match db::events::update_event(
            &state.db,
            id,
            req.label.as_deref().map(str::trim).filter(|value| !value.is_empty()),
            req.amount,
            req.expected_date.as_deref(),
            req.account_id,
        )
        .await
        {
            Ok(true) => {
                invalidate_data_caches(&state).await;
                Json(serde_json::json!({"ok": true})).into_response()
            }
            Ok(false) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal", "message": "Update failed."})),
            )
                .into_response(),
            Err(e) => {
                tracing::error!("failed to update event {id}: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "internal", "message": "Update failed."})),
                )
                    .into_response()
            }
        },
    }
}

#[derive(Deserialize)]
struct AccountRequest {
    name: String,
    kind: Option<String>,
    balance: Option<f64>,
    is_primary: Option<bool>,
    notes: Option<String>,
}

async fn create_account(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AccountRequest>,
) -> impl IntoResponse {
    if req.name.trim().is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "validation_error", "message": "Account name is required."})),
        ).into_response();
    }

    match db::accounts::create_account(
        &state.db,
        &req.name,
        req.kind.as_deref().unwrap_or("cash"),
        req.balance,
        req.is_primary.unwrap_or(false),
        req.notes.as_deref(),
    )
    .await
    {
        Ok(id) => {
            invalidate_data_caches(&state).await;
            (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response()
        }
        Err(e) => {
            tracing::error!("failed to create account: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    serde_json::json!({"error": "internal", "message": "Failed to create account."}),
                ),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct StreamRequest {
    name: String,
    kind: Option<String>,
    description: Option<String>,
    default_account_id: Option<i64>,
    schedule_amount: Option<f64>,
    schedule_frequency: Option<String>,
    due_day: Option<i32>,
    start_date: Option<String>,
}

async fn update_account(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<AccountRequest>,
) -> impl IntoResponse {
    if req.name.trim().is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "validation_error", "message": "Account name is required."})),
        ).into_response();
    }

    match db::accounts::update_account(
        &state.db,
        id,
        &req.name,
        req.kind.as_deref().unwrap_or("cash"),
        req.balance,
        req.is_primary.unwrap_or(false),
        req.notes.as_deref(),
    )
    .await
    {
        Ok(true) => {
            invalidate_data_caches(&state).await;
            Json(serde_json::json!({"ok": true})).into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "not_found", "message": "Account not found."})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("failed to update account {id}: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal", "message": "Failed to update account."})),
            )
                .into_response()
        }
    }
}

async fn create_stream(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StreamRequest>,
) -> impl IntoResponse {
    if req.name.trim().is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "validation_error", "message": "Stream name is required."})),
        )
            .into_response();
    }

    if let Some(due_day) = req.due_day {
        if !(1..=31).contains(&due_day) {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": "validation_error", "message": "Due day must be between 1 and 31."})),
            )
                .into_response();
        }
    }

    if let Some(start_date) = req.start_date.as_deref() {
        if !start_date.trim().is_empty()
            && chrono::NaiveDate::parse_from_str(start_date, "%Y-%m-%d").is_err()
        {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": "validation_error", "message": "Invalid start date format. Use YYYY-MM-DD."})),
            )
                .into_response();
        }
    }

    if let Some(account_id) = req.default_account_id {
        match db::accounts::account_exists(&state.db, account_id).await {
            Ok(true) => {}
            Ok(false) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "account_not_found", "message": "Account does not exist."})),
                )
                    .into_response();
            }
            Err(e) => {
                tracing::error!("account existence check failed: {e}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "internal", "message": "Failed to save stream."})),
                )
                    .into_response();
            }
        }
    }

    match db::streams::create_stream(
        &state.db,
        &req.name,
        req.kind.as_deref().unwrap_or("manual"),
        req.description.as_deref(),
        req.default_account_id,
        req.schedule_amount,
        req.schedule_frequency.as_deref(),
        req.due_day,
        req.start_date.as_deref(),
    )
    .await
    {
        Ok(id) => {
            invalidate_data_caches(&state).await;
            (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response()
        }
        Err(e) => {
            tracing::error!("failed to create stream: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    serde_json::json!({"error": "internal", "message": "Failed to create stream."}),
                ),
            )
                .into_response()
        }
    }
}

async fn update_stream(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<StreamRequest>,
) -> impl IntoResponse {
    if req.name.trim().is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "validation_error", "message": "Stream name is required."})),
        )
            .into_response();
    }

    match db::streams::update_stream(
        &state.db,
        id,
        &req.name,
        req.kind.as_deref().unwrap_or("manual"),
        req.description.as_deref(),
        req.default_account_id,
        req.schedule_amount,
        req.schedule_frequency.as_deref(),
        req.due_day,
        req.start_date.as_deref(),
    )
    .await
    {
        Ok(true) => {
            invalidate_data_caches(&state).await;
            Json(serde_json::json!({"ok": true})).into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "not_found", "message": "Stream not found."})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("failed to update stream {id}: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    serde_json::json!({"error": "internal", "message": "Failed to update stream."}),
                ),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct ViewRequest {
    name: String,
    description: Option<String>,
    stream_ids: Option<Vec<i64>>,
}

async fn create_view(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ViewRequest>,
) -> impl IntoResponse {
    if req.name.trim().is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "validation_error", "message": "View name is required."})),
        )
            .into_response();
    }

    match db::streams::create_view(
        &state.db,
        &req.name,
        req.description.as_deref(),
        req.stream_ids.as_deref().unwrap_or(&[]),
    )
    .await
    {
        Ok(id) => {
            invalidate_data_caches(&state).await;
            (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response()
        }
        Err(e) => {
            tracing::error!("failed to create view: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal", "message": "Failed to create view."})),
            )
                .into_response()
        }
    }
}

async fn update_view(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<ViewRequest>,
) -> impl IntoResponse {
    if req.name.trim().is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": "validation_error", "message": "View name is required."})),
        )
            .into_response();
    }

    match db::streams::update_view(
        &state.db,
        id,
        &req.name,
        req.description.as_deref(),
        req.stream_ids.as_deref().unwrap_or(&[]),
    )
    .await
    {
        Ok(true) => {
            invalidate_data_caches(&state).await;
            Json(serde_json::json!({"ok": true})).into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "not_found", "message": "View not found."})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("failed to update view {id}: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal", "message": "Failed to update view."})),
            )
                .into_response()
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
        Ok(()) => {
            invalidate_data_caches(&state).await;
            Json(serde_json::json!({"ok": true}))
        }
        Err(e) => {
            tracing::error!("failed to set cash balance: {e}");
            Json(serde_json::json!({"error": "internal", "message": "Failed to set balance."}))
        }
    }
}

/// POST /api/sync/balance — pull current balance from Monarch and store as starting cash.
async fn sync_monarch_balance(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let connection_id = match crate::db::integrations::ensure_connection(
        &state.db, "monarch", "Monarch", "monarch", None,
    )
    .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("failed to ensure Monarch connection: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal", "message": "Failed to prepare Monarch integration."})),
            )
                .into_response();
        }
    };

    let credential = match crate::db::integrations::get_or_bootstrap_monarch_credential(
        &state.db,
        connection_id,
    )
    .await
    {
        Ok(credential) => credential,
        Err(e) => {
            tracing::error!("failed to load Monarch credentials: {e}");
            return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "config", "message": "Monarch credentials are not configured yet."})),
                )
                    .into_response();
        }
    };

    let client = match crate::monarch::client::MonarchClient::with_token(&credential.access_token) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("failed to create Monarch client: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "config", "message": "Failed to create Monarch client."})),
            )
                .into_response();
        }
    };

    let account_id = credential.default_account_id;

    match client.get_adjusted_balance(&account_id).await {
        Ok((balance, adjusted, pending_total)) => {
            let metadata = serde_json::json!({
                "reported_balance": balance.current_balance,
                "pending_total": pending_total,
                "adjusted_balance": adjusted,
                "account": balance.display_name,
                "mask": balance.mask,
            });
            match db::accounts::set_primary_balance(
                &state.db,
                adjusted,
                "monarch",
                Some(&balance.id),
                Some(&metadata.to_string()),
                Some(&balance.updated_at),
            )
            .await
            {
                Ok(()) => {
                    invalidate_data_caches(&state).await;
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
            )
                .into_response()
        }
    }
}
