use std::{fmt::Display, str::FromStr};

use axum::{
    Json,
    extract::{Extension, Path, Query},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use libsql::Connection;
use serde::{Deserialize, Deserializer};
use serde_json::json;
use time::OffsetDateTime;

use crate::{
    db::AppContext,
    templates::{self, CanvasTemplate, ForecastTemplate, StreamsTemplate},
};

use super::{
    AccountDraft, AmountCertainty, CanvasStreamView, Direction, EventDraft, EventPatch,
    EventStatus, FinanceError, FinanceRepository, ForecastQuery, IsoDate, Patch, ProjectionWindow,
    ScheduleDraft, ScheduleFrequency, StreamConfigView, StreamDraft, StreamScheduleView,
    StreamViewDraft,
};

const FORECAST_DAYS: i64 = 180;
const PAGE_HISTORY_DAYS: i64 = 120;
const PROJECTION_DAYS: i64 = 365;

#[derive(Debug, Default, Deserialize)]
pub struct ForecastParams {
    from: Option<String>,
    through: Option<String>,
    stream_id: Option<i64>,
    view_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateEventRequest {
    stream_id: i64,
    account_id: Option<i64>,
    label: String,
    expected_date: String,
    amount: f64,
}

#[derive(Debug, Deserialize)]
pub struct UpdateEventRequest {
    #[serde(default)]
    label: WireField<String>,
    #[serde(default)]
    amount: WireField<f64>,
    #[serde(default)]
    expected_date: WireField<String>,
    #[serde(default)]
    account_id: WireField<i64>,
    #[serde(default)]
    notes: WireField<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReconcileRequest {
    actual_date: String,
    amount: f64,
}

#[derive(Debug, Deserialize)]
pub struct AccountRequest {
    name: String,
    kind: Option<String>,
    #[serde(default)]
    balance: WireField<f64>,
    is_primary: Option<bool>,
    #[serde(default)]
    notes: WireField<String>,
}

#[derive(Debug, Deserialize)]
pub struct StreamRequest {
    name: String,
    kind: Option<String>,
    amount_certainty: Option<String>,
    #[serde(default)]
    description: WireField<String>,
    #[serde(default)]
    default_account_id: WireField<i64>,
    schedule_id: Option<i64>,
    #[serde(default)]
    schedule_amount: WireField<f64>,
    #[serde(default)]
    schedule_frequency: WireField<String>,
    #[serde(default)]
    due_day: WireField<i64>,
    #[serde(default)]
    start_date: WireField<String>,
    #[serde(default)]
    end_date: WireField<String>,
    #[serde(default)]
    schedules: WireField<Vec<ScheduleRequest>>,
}

#[derive(Debug, Deserialize)]
pub struct ScheduleRequest {
    id: Option<i64>,
    account_id: Option<i64>,
    label: Option<String>,
    amount: f64,
    frequency: String,
    #[serde(alias = "due_day")]
    day_of_month: Option<i64>,
    start_date: String,
    end_date: Option<String>,
    metadata: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ViewRequest {
    name: String,
    #[serde(default)]
    description: WireField<String>,
    stream_ids: Option<Vec<i64>>,
    is_default: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct SetCashRequest {
    amount: f64,
    /// Browser-local calendar date. Older API clients may omit it and retain
    /// the UTC fallback used before this field existed.
    as_of_date: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
enum WireField<T> {
    #[default]
    Missing,
    Null,
    Value(T),
}

struct ApiProblem {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl IntoResponse for ApiProblem {
    fn into_response(self) -> Response {
        api_error(self.status, self.code, &self.message)
    }
}

impl<'de, T> Deserialize<'de> for WireField<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match Option::<T>::deserialize(deserializer)? {
            Some(value) => Self::Value(value),
            None => Self::Null,
        })
    }
}

pub async fn streams_page(Extension(context): Extension<AppContext>) -> Response {
    let connection = match page_connection(&context, "load Streams page").await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let repository = FinanceRepository::new(&connection);
    let accounts = match repository.list_accounts().await {
        Ok(accounts) => accounts,
        Err(error) => return page_storage_error("load Streams accounts", error),
    };
    let streams = match repository.list_streams().await {
        Ok(streams) => streams,
        Err(error) => return page_storage_error("load Streams", error),
    };
    let views = match repository.list_view_editors().await {
        Ok(views) => views,
        Err(error) => return page_storage_error("load stream views", error),
    };

    templates::streams_response(StreamsTemplate {
        title: "Trust Deeds - Streams".into(),
        accounts,
        streams,
        views,
    })
}

pub async fn forecast_page(Extension(context): Extension<AppContext>) -> Response {
    let connection = match page_connection(&context, "load Timeline page").await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let repository = FinanceRepository::new(&connection);
    let accounts = match repository.list_accounts().await {
        Ok(accounts) => accounts,
        Err(error) => return page_storage_error("load Timeline accounts", error),
    };
    let streams = match repository.list_streams().await {
        Ok(streams) => streams,
        Err(error) => return page_storage_error("load Timeline streams", error),
    };
    let views = match repository.list_view_summaries().await {
        Ok(views) => views,
        Err(error) => return page_storage_error("load Timeline views", error),
    };
    let default_view_id = match repository.default_view_id().await {
        Ok(view_id) => view_id,
        Err(error) => return page_storage_error("load default Timeline view", error),
    };
    let selected_view_id = default_view_id
        .or_else(|| views.first().map(|view| view.id))
        .unwrap_or(0);
    let default_stream_id = streams.first().map(|stream| stream.id).unwrap_or(0);
    let today = server_today();
    let from = match today.add_days(-PAGE_HISTORY_DAYS) {
        Ok(date) => date,
        Err(error) => return page_storage_error("build Timeline window", error),
    };
    let through = match today.add_days(PROJECTION_DAYS) {
        Ok(date) => date,
        Err(error) => return page_storage_error("build Timeline window", error),
    };
    let forecast = match repository
        .compute_forecast(ForecastQuery {
            from,
            through,
            today,
            stream_id: None,
            view_id: (selected_view_id != 0).then_some(selected_view_id),
        })
        .await
    {
        Ok(forecast) => forecast,
        Err(error) => return page_storage_error("compute initial Timeline", error),
    };

    templates::forecast_response(ForecastTemplate {
        title: "Trust Deeds - Timeline".into(),
        accounts,
        streams,
        views,
        forecast,
        selected_view_id,
        default_stream_id,
    })
}

pub async fn canvas_page(Extension(context): Extension<AppContext>) -> Response {
    let connection = match context.connection().await {
        Ok(connection) => connection,
        Err(error) => return canvas_page_error("open Canvas database", error),
    };
    let streams = match FinanceRepository::new(&connection)
        .list_canvas_streams()
        .await
    {
        Ok(streams) => streams,
        Err(error) => return canvas_page_error("load Canvas streams", error),
    };
    let default_stream_id = default_canvas_stream_id(&streams);

    templates::canvas_response(CanvasTemplate {
        title: "Trust Deeds - Canvas".into(),
        streams,
        default_stream_id,
    })
}

fn default_canvas_stream_id(streams: &[CanvasStreamView]) -> i64 {
    streams
        .iter()
        .find(|stream| stream.name == "Trust Deeds" || stream.kind == "tmo_trust")
        .or_else(|| streams.first())
        .map_or(0, |stream| stream.id)
}

pub async fn get_forecast(
    Extension(context): Extension<AppContext>,
    Query(params): Query<ForecastParams>,
) -> Response {
    let today = server_today();
    let from = match parse_query_date("from", params.from.as_deref(), today) {
        Ok(date) => date,
        Err(problem) => return problem.into_response(),
    };
    let default_through = match today.add_days(FORECAST_DAYS) {
        Ok(date) => date,
        Err(error) => return api_storage_error("build forecast window", error),
    };
    let through = match parse_query_date("through", params.through.as_deref(), default_through) {
        Ok(date) => date,
        Err(problem) => return problem.into_response(),
    };
    if through < from {
        return api_error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "'through' must be on or after 'from'.",
        );
    }

    let connection = match api_connection(&context, "load forecast").await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let repository = FinanceRepository::new(&connection);

    if let Some(stream_id) = params.stream_id {
        let streams = match repository.list_streams().await {
            Ok(streams) => streams,
            Err(error) => return api_storage_error("validate forecast stream", error),
        };
        if !streams.iter().any(|stream| stream.id == stream_id) {
            return api_error(
                StatusCode::NOT_FOUND,
                "stream_not_found",
                "Stream does not exist.",
            );
        }
    }
    if let Some(view_id) = params.view_id {
        let views = match repository.list_view_summaries().await {
            Ok(views) => views,
            Err(error) => return api_storage_error("validate forecast view", error),
        };
        if !views.iter().any(|view| view.id == view_id) {
            return api_error(
                StatusCode::NOT_FOUND,
                "view_not_found",
                "View does not exist.",
            );
        }
    }

    match repository
        .compute_forecast(ForecastQuery {
            from,
            through,
            today,
            stream_id: params.stream_id,
            view_id: params.view_id,
        })
        .await
    {
        Ok(Some(forecast)) => Json(forecast).into_response(),
        Ok(None) => api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "no_starting_balance",
            "Set your current cash balance first.",
        ),
        Err(error) => api_storage_error("compute forecast", error),
    }
}

pub async fn create_event(
    Extension(context): Extension<AppContext>,
    Json(request): Json<CreateEventRequest>,
) -> Response {
    let expected_date = match parse_mutation_date("date", &request.expected_date) {
        Ok(date) => date,
        Err(problem) => return problem.into_response(),
    };
    let connection = match api_connection(&context, "create event").await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let repository = FinanceRepository::new(&connection);
    let draft = EventDraft {
        stream_id: request.stream_id,
        account_id: request.account_id,
        label: request.label,
        expected_date,
        amount: request.amount,
        status: EventStatus::Projected,
        metadata: None,
        notes: None,
    };
    match repository.create_manual_event(&draft).await {
        Ok(id) => (StatusCode::CREATED, Json(json!({ "id": id }))).into_response(),
        Err(error) => finance_error_response("create event", error),
    }
}

pub async fn update_event(
    Extension(context): Extension<AppContext>,
    Path(id): Path<i64>,
    Json(request): Json<UpdateEventRequest>,
) -> Response {
    let expected_date = match request.expected_date {
        WireField::Missing => Patch::Keep,
        WireField::Null => Patch::Clear,
        WireField::Value(value) => match parse_mutation_date("date", &value) {
            Ok(date) => Patch::Set(date),
            Err(problem) => return problem.into_response(),
        },
    };
    let patch = EventPatch {
        label: wire_patch(request.label),
        expected_date,
        amount: wire_patch(request.amount),
        account_id: wire_patch(request.account_id),
        notes: wire_patch(request.notes),
    };
    let connection = match api_connection(&context, "update event").await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let repository = FinanceRepository::new(&connection);
    match repository.patch_event(id, &patch).await {
        Ok(()) => ok_response(),
        Err(error) => finance_error_response("update event", error),
    }
}

pub async fn delete_event(
    Extension(context): Extension<AppContext>,
    Path(id): Path<i64>,
) -> Response {
    let connection = match api_connection(&context, "delete event").await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let repository = FinanceRepository::new(&connection);
    match repository.remove_event(id).await {
        Ok(()) => ok_response(),
        Err(error) => finance_error_response("delete event", error),
    }
}

pub async fn reconcile_event(
    Extension(context): Extension<AppContext>,
    Path(id): Path<i64>,
    Json(request): Json<ReconcileRequest>,
) -> Response {
    let actual_date = match parse_mutation_date("actual date", &request.actual_date) {
        Ok(date) => date,
        Err(problem) => return problem.into_response(),
    };
    let connection = match api_connection(&context, "reconcile event").await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let repository = FinanceRepository::new(&connection);
    match repository
        .reconcile_event(id, actual_date, request.amount)
        .await
    {
        Ok(()) => ok_response(),
        Err(error) => finance_error_response("reconcile event", error),
    }
}

pub async fn create_account(
    Extension(context): Extension<AppContext>,
    Json(request): Json<AccountRequest>,
) -> Response {
    save_account(context, None, request).await
}

pub async fn update_account(
    Extension(context): Extension<AppContext>,
    Path(id): Path<i64>,
    Json(request): Json<AccountRequest>,
) -> Response {
    save_account(context, Some(id), request).await
}

async fn save_account(context: AppContext, id: Option<i64>, request: AccountRequest) -> Response {
    let operation = if id.is_some() {
        "update account"
    } else {
        "create account"
    };
    let connection = match api_connection(&context, operation).await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let repository = FinanceRepository::new(&connection);
    let existing = if let Some(id) = id {
        let accounts = match repository.list_accounts().await {
            Ok(accounts) => accounts,
            Err(error) => return api_storage_error("load account for update", error),
        };
        let Some(account) = accounts.into_iter().find(|account| account.id == id) else {
            return api_error(StatusCode::NOT_FOUND, "not_found", "Account not found.");
        };
        Some(account)
    } else {
        None
    };
    let existing_balance_date = match existing
        .as_ref()
        .and_then(|account| account.balance_as_of_date.as_deref())
        .map(|date| persisted_date("account balance as-of date", date))
        .transpose()
    {
        Ok(date) => date,
        Err(error) => return finance_error_response(operation, error),
    };
    let (balance, balance_as_of_date) = match request.balance {
        WireField::Missing => (
            existing.as_ref().and_then(|account| account.balance),
            existing_balance_date,
        ),
        WireField::Null => (None, None),
        WireField::Value(balance)
            if existing.as_ref().and_then(|account| account.balance) == Some(balance) =>
        {
            (Some(balance), existing_balance_date)
        }
        WireField::Value(balance) => (Some(balance), Some(server_today())),
    };
    let notes = match request.notes {
        WireField::Missing => existing.as_ref().and_then(|account| account.notes.clone()),
        WireField::Null => None,
        WireField::Value(notes) => Some(notes),
    };
    let draft = AccountDraft {
        id,
        name: request.name,
        kind: request.kind.unwrap_or_else(|| {
            existing
                .as_ref()
                .map(|account| account.kind.clone())
                .unwrap_or_else(|| "cash".into())
        }),
        balance,
        balance_as_of_date,
        is_primary: request.is_primary.unwrap_or_else(|| {
            existing
                .as_ref()
                .is_some_and(|account| account.is_primary == 1)
        }),
        notes,
    };
    match repository.save_account(&draft).await {
        Ok(saved_id) if id.is_none() => {
            (StatusCode::CREATED, Json(json!({ "id": saved_id }))).into_response()
        }
        Ok(_) => ok_response(),
        Err(error) => finance_error_response(operation, error),
    }
}

pub async fn create_stream(
    Extension(context): Extension<AppContext>,
    Json(request): Json<StreamRequest>,
) -> Response {
    save_stream(context, None, request).await
}

pub async fn update_stream(
    Extension(context): Extension<AppContext>,
    Path(id): Path<i64>,
    Json(request): Json<StreamRequest>,
) -> Response {
    save_stream(context, Some(id), request).await
}

async fn save_stream(context: AppContext, id: Option<i64>, request: StreamRequest) -> Response {
    let operation = if id.is_some() {
        "update stream"
    } else {
        "create stream"
    };
    let connection = match api_connection(&context, operation).await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let repository = FinanceRepository::new(&connection);
    let streams = if id.is_some() {
        match repository.list_streams().await {
            Ok(streams) => streams,
            Err(error) => return api_storage_error("load stream for update", error),
        }
    } else {
        Vec::new()
    };
    let existing = id.and_then(|id| streams.iter().find(|stream| stream.id == id));
    if id.is_some() && existing.is_none() {
        return api_error(StatusCode::NOT_FOUND, "not_found", "Stream not found.");
    }
    let today = server_today();
    let draft = match build_stream_draft(id, request, existing, today) {
        Ok(draft) => draft,
        Err(error) => return finance_error_response(operation, error),
    };
    let through = match today.add_days(PROJECTION_DAYS) {
        Ok(date) => date,
        Err(error) => return api_storage_error("build stream projection window", error),
    };
    let projection_window = match ProjectionWindow::new(today, through) {
        Ok(window) => window,
        Err(error) => return api_storage_error("build stream projection window", error),
    };
    match repository.save_stream(&draft, projection_window).await {
        Ok(saved_id) if id.is_none() => {
            (StatusCode::CREATED, Json(json!({ "id": saved_id }))).into_response()
        }
        Ok(_) => ok_response(),
        Err(error) => finance_error_response(operation, error),
    }
}

pub async fn delete_stream(
    Extension(context): Extension<AppContext>,
    Path(id): Path<i64>,
) -> Response {
    let connection = match api_connection(&context, "delete stream").await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let repository = FinanceRepository::new(&connection);
    match repository.delete_stream(id).await {
        Ok(()) => ok_response(),
        Err(error) => finance_error_response("delete stream", error),
    }
}

pub async fn create_view(
    Extension(context): Extension<AppContext>,
    Json(request): Json<ViewRequest>,
) -> Response {
    save_view(context, None, request).await
}

pub async fn update_view(
    Extension(context): Extension<AppContext>,
    Path(id): Path<i64>,
    Json(request): Json<ViewRequest>,
) -> Response {
    save_view(context, Some(id), request).await
}

async fn save_view(context: AppContext, id: Option<i64>, request: ViewRequest) -> Response {
    let operation = if id.is_some() {
        "update view"
    } else {
        "create view"
    };
    let connection = match api_connection(&context, operation).await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let repository = FinanceRepository::new(&connection);
    let existing = if let Some(id) = id {
        let views = match repository.list_view_editors().await {
            Ok(views) => views,
            Err(error) => return api_storage_error("load view for update", error),
        };
        let Some(view) = views.into_iter().find(|view| view.id == id) else {
            return api_error(StatusCode::NOT_FOUND, "not_found", "View not found.");
        };
        Some(view)
    } else {
        None
    };
    let description = match request.description {
        WireField::Missing => existing.as_ref().and_then(|view| view.description.clone()),
        WireField::Null => None,
        WireField::Value(value) => Some(value),
    };
    let draft = StreamViewDraft {
        id,
        name: request.name,
        description,
        is_default: request
            .is_default
            .unwrap_or_else(|| existing.as_ref().is_some_and(|view| view.is_default == 1)),
        stream_ids: request.stream_ids.unwrap_or_else(|| {
            existing
                .as_ref()
                .map(|view| {
                    view.members
                        .iter()
                        .filter(|member| member.included)
                        .map(|member| member.stream_id)
                        .collect()
                })
                .unwrap_or_default()
        }),
    };
    match repository.save_view(&draft).await {
        Ok(saved_id) if id.is_none() => {
            (StatusCode::CREATED, Json(json!({ "id": saved_id }))).into_response()
        }
        Ok(_) => ok_response(),
        Err(error) => finance_error_response(operation, error),
    }
}

pub async fn set_cash_balance(
    Extension(context): Extension<AppContext>,
    Json(request): Json<SetCashRequest>,
) -> Response {
    let as_of_date = match request.as_of_date {
        Some(value) => match parse_mutation_date("cash as-of date", &value) {
            Ok(date) => date,
            Err(problem) => return problem.into_response(),
        },
        None => server_today(),
    };
    let connection = match api_connection(&context, "set cash balance").await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let repository = FinanceRepository::new(&connection);
    match repository
        .set_starting_balance(request.amount, as_of_date, "manual", None, None)
        .await
    {
        Ok(()) => ok_response(),
        Err(error) => finance_error_response("set cash balance", error),
    }
}

fn build_stream_draft(
    id: Option<i64>,
    request: StreamRequest,
    existing: Option<&StreamConfigView>,
    today: IsoDate,
) -> Result<StreamDraft, FinanceError> {
    let StreamRequest {
        name,
        kind,
        amount_certainty,
        description,
        default_account_id,
        schedule_id,
        schedule_amount,
        schedule_frequency,
        due_day,
        start_date,
        end_date,
        schedules,
    } = request;
    let kind = match kind {
        Some(kind) => kind.trim().to_owned(),
        None => existing
            .map(|stream| stream.kind.clone())
            .unwrap_or_else(|| "manual".into()),
    };
    let preserves_imported_kind = existing.is_some_and(|stream| stream.kind == kind);
    let (stream_type, direction) = if preserves_imported_kind {
        let stream = existing.expect("existing stream checked above");
        let direction = Direction::from_str(&stream.direction).map_err(FinanceError::from)?;
        (stream.stream_type.clone(), direction)
    } else {
        (
            stream_type_for_kind(&kind)?.to_owned(),
            Direction::for_kind(&kind),
        )
    };
    let certainty = amount_certainty
        .or_else(|| existing.map(|stream| stream.amount_certainty.clone()))
        .map(|value| {
            AmountCertainty::from_str(value.trim()).map_err(|_| {
                FinanceError::validation("Amount certainty must be 'known' or 'estimated'.")
            })
        })
        .transpose()?
        .unwrap_or_else(|| AmountCertainty::for_kind(&kind));
    let description = match description {
        WireField::Missing => existing.and_then(|stream| stream.description.clone()),
        WireField::Null => None,
        WireField::Value(value) => Some(value),
    };
    let default_account_changed = !matches!(default_account_id, WireField::Missing);
    let default_account_id = match default_account_id {
        WireField::Missing => existing.and_then(|stream| stream.default_account_id),
        WireField::Null => None,
        WireField::Value(value) => Some(value),
    };
    let prior_schedules = existing
        .map(|stream| stream.schedules.as_slice())
        .unwrap_or_default();
    let schedules = match schedules {
        WireField::Value(schedules) => schedules
            .into_iter()
            .map(schedule_from_request)
            .collect::<Result<Vec<_>, _>>()?,
        WireField::Null => Vec::new(),
        WireField::Missing => flattened_schedules(
            &name,
            prior_schedules,
            schedule_id,
            schedule_amount,
            schedule_frequency,
            due_day,
            start_date,
            end_date,
            default_account_id,
            default_account_changed,
            today,
        )?,
    };

    Ok(StreamDraft {
        id,
        name,
        stream_type,
        direction,
        kind,
        amount_certainty: certainty,
        description,
        default_account_id,
        configuration: existing.and_then(|stream| stream.configuration.clone()),
        parent_id: existing.and_then(|stream| stream.parent_id),
        schedules,
    })
}

#[allow(clippy::too_many_arguments)]
fn flattened_schedules(
    stream_name: &str,
    existing: &[StreamScheduleView],
    schedule_id: Option<i64>,
    amount: WireField<f64>,
    frequency: WireField<String>,
    day_of_month: WireField<i64>,
    start_date: WireField<String>,
    end_date: WireField<String>,
    default_account_id: Option<i64>,
    default_account_changed: bool,
    today: IsoDate,
) -> Result<Vec<ScheduleDraft>, FinanceError> {
    let has_flattened_update = schedule_id.is_some()
        || !matches!(amount, WireField::Missing)
        || !matches!(frequency, WireField::Missing)
        || !matches!(day_of_month, WireField::Missing)
        || !matches!(start_date, WireField::Missing)
        || !matches!(end_date, WireField::Missing);
    if !has_flattened_update {
        return existing.iter().map(schedule_from_view).collect();
    }

    let first = existing.first();
    let frequency = match frequency {
        WireField::Null => None,
        WireField::Value(value) if value.trim().is_empty() => None,
        WireField::Value(value) => Some(parse_frequency(&value)?),
        WireField::Missing => first
            .map(|schedule| persisted_frequency(&schedule.frequency))
            .transpose()?,
    };
    let mut drafts = Vec::new();
    if let Some(frequency) = frequency {
        let amount = match amount {
            WireField::Missing => first.map(|schedule| schedule.amount).unwrap_or(0.0),
            WireField::Null => 0.0,
            WireField::Value(value) => value,
        };
        let mut day_of_month = match day_of_month {
            WireField::Missing => first.and_then(|schedule| schedule.day_of_month),
            WireField::Null => None,
            WireField::Value(value) => Some(value),
        };
        if frequency == ScheduleFrequency::Monthly && day_of_month.is_none() {
            day_of_month = Some(1);
        }
        let day_of_month = day_of_month.map(parse_day_of_month).transpose()?;
        let start_date = match start_date {
            WireField::Missing => first
                .map(|schedule| persisted_date("schedule start", &schedule.start_date))
                .transpose()?
                .unwrap_or(today),
            WireField::Null => today,
            WireField::Value(value) => parse_schedule_date("start", &value)?,
        };
        let end_date = match end_date {
            WireField::Missing => first
                .and_then(|schedule| schedule.end_date.as_deref())
                .map(|value| persisted_date("schedule end", value))
                .transpose()?,
            WireField::Null => None,
            WireField::Value(value) if value.trim().is_empty() => None,
            WireField::Value(value) => Some(parse_schedule_date("end", &value)?),
        };
        let account_id = if default_account_changed {
            default_account_id
        } else {
            first
                .and_then(|schedule| schedule.account_id)
                .or(default_account_id)
        };
        drafts.push(ScheduleDraft {
            id: schedule_id.or_else(|| first.map(|schedule| schedule.id)),
            account_id,
            label: first
                .and_then(|schedule| schedule.label.clone())
                .or_else(|| Some(format!("{} due", stream_name.trim()))),
            amount,
            frequency,
            day_of_month,
            start_date,
            end_date,
            metadata: first.and_then(|schedule| schedule.metadata.clone()),
        });
    }

    // The legacy form edits only the flattened first schedule. Every other
    // schedule is deliberately round-tripped so an ordinary save cannot
    // erase invisible recurrence rules or their stable source IDs.
    drafts.extend(
        existing
            .iter()
            .skip(1)
            .map(schedule_from_view)
            .collect::<Result<Vec<_>, _>>()?,
    );
    Ok(drafts)
}

fn schedule_from_request(request: ScheduleRequest) -> Result<ScheduleDraft, FinanceError> {
    Ok(ScheduleDraft {
        id: request.id,
        account_id: request.account_id,
        label: request.label,
        amount: request.amount,
        frequency: parse_frequency(&request.frequency)?,
        day_of_month: request.day_of_month.map(parse_day_of_month).transpose()?,
        start_date: parse_schedule_date("start", &request.start_date)?,
        end_date: request
            .end_date
            .as_deref()
            .map(|value| parse_schedule_date("end", value))
            .transpose()?,
        metadata: request.metadata,
    })
}

fn schedule_from_view(schedule: &StreamScheduleView) -> Result<ScheduleDraft, FinanceError> {
    Ok(ScheduleDraft {
        id: Some(schedule.id),
        account_id: schedule.account_id,
        label: schedule.label.clone(),
        amount: schedule.amount,
        frequency: persisted_frequency(&schedule.frequency)?,
        day_of_month: schedule
            .day_of_month
            .map(|day| {
                u8::try_from(day).map_err(|error| {
                    FinanceError::from(
                        anyhow::Error::new(error).context("decode persisted schedule day"),
                    )
                })
            })
            .transpose()?,
        start_date: persisted_date("schedule start", &schedule.start_date)?,
        end_date: schedule
            .end_date
            .as_deref()
            .map(|value| persisted_date("schedule end", value))
            .transpose()?,
        metadata: schedule.metadata.clone(),
    })
}

fn stream_type_for_kind(kind: &str) -> Result<&'static str, FinanceError> {
    match kind {
        "manual_income" => Ok("manual_income"),
        "manual_expense" => Ok("manual_expense"),
        "credit_card" => Ok("credit_card_due"),
        "tmo_trust" => Ok("mortgage_portfolio"),
        "manual" => Ok("manual"),
        _ => Err(FinanceError::validation(
            "Stream kind must be manual, manual_income, manual_expense, credit_card, or tmo_trust.",
        )),
    }
}

fn parse_frequency(value: &str) -> Result<ScheduleFrequency, FinanceError> {
    ScheduleFrequency::from_str(value.trim()).map_err(|_| {
        FinanceError::validation(
            "Schedule frequency must be monthly, semimonthly, biweekly, weekly, annual, or one_time.",
        )
    })
}

fn persisted_frequency(value: &str) -> Result<ScheduleFrequency, FinanceError> {
    ScheduleFrequency::from_str(value)
        .map_err(|error| FinanceError::from(error.context("decode persisted schedule frequency")))
}

fn parse_day_of_month(value: i64) -> Result<u8, FinanceError> {
    if !(1..=31).contains(&value) {
        return Err(FinanceError::validation(
            "Due day must be between 1 and 31.",
        ));
    }
    Ok(value as u8)
}

fn parse_schedule_date(label: &str, value: &str) -> Result<IsoDate, FinanceError> {
    value.trim().parse().map_err(|_| {
        FinanceError::validation(format!("Invalid schedule {label} date. Use YYYY-MM-DD."))
    })
}

fn persisted_date(label: &str, value: &str) -> Result<IsoDate, FinanceError> {
    value.parse().map_err(|error: anyhow::Error| {
        FinanceError::from(error.context(format!("decode {label}")))
    })
}

fn wire_patch<T>(field: WireField<T>) -> Patch<T> {
    match field {
        WireField::Missing => Patch::Keep,
        WireField::Null => Patch::Clear,
        WireField::Value(value) => Patch::Set(value),
    }
}

fn server_today() -> IsoDate {
    let today = OffsetDateTime::now_utc().date();
    IsoDate::new(today.year(), today.month() as u8, today.day())
        .expect("the server UTC date is always a valid ISO date")
}

fn parse_query_date(
    field: &str,
    value: Option<&str>,
    default: IsoDate,
) -> Result<IsoDate, ApiProblem> {
    let Some(value) = value else {
        return Ok(default);
    };
    value.parse().map_err(|_| ApiProblem {
        status: StatusCode::BAD_REQUEST,
        code: "bad_request",
        message: format!("Invalid '{field}' date format. Use YYYY-MM-DD."),
    })
}

fn parse_mutation_date(label: &str, value: &str) -> Result<IsoDate, ApiProblem> {
    value.parse().map_err(|_| ApiProblem {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        code: "validation_error",
        message: format!("Invalid {label} format. Use YYYY-MM-DD."),
    })
}

async fn page_connection(
    context: &AppContext,
    operation: &'static str,
) -> Result<Connection, Response> {
    context
        .connection()
        .await
        .map_err(|error| page_storage_error(operation, error))
}

async fn api_connection(
    context: &AppContext,
    operation: &'static str,
) -> Result<Connection, Response> {
    context
        .connection()
        .await
        .map_err(|error| api_storage_error(operation, error))
}

fn finance_error_response(operation: &'static str, error: FinanceError) -> Response {
    match error {
        FinanceError::Validation(message) => api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_error",
            &message,
        ),
        FinanceError::NotFound(message) => api_error(StatusCode::NOT_FOUND, "not_found", &message),
        FinanceError::Conflict(message) => api_error(StatusCode::CONFLICT, "conflict", &message),
        FinanceError::Storage(error) => api_storage_error(operation, error),
    }
}

fn api_error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({ "error": code, "message": message }))).into_response()
}

fn ok_response() -> Response {
    Json(json!({ "ok": true })).into_response()
}

fn api_storage_error(operation: &'static str, error: impl Display) -> Response {
    tracing::error!(%error, operation, "finance storage unavailable");
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "service_unavailable",
        "Finance data is temporarily unavailable. Try again.",
    )
}

fn page_storage_error(operation: &'static str, error: impl Display) -> Response {
    tracing::error!(%error, operation, "finance page storage unavailable");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Html(
            "<main><h1>Temporarily unavailable</h1><p>Finance data could not be loaded. Try again.</p></main>",
        ),
    )
        .into_response()
}

fn canvas_page_error(operation: &'static str, error: impl Display) -> Response {
    tracing::error!(%error, operation, "Canvas page storage unavailable");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Html(
            "<main><h1>Canvas unavailable</h1><p>Canvas data could not be loaded. Try again.</p></main>",
        ),
    )
        .into_response()
}

#[cfg(all(test, feature = "local-db"))]
mod tests {
    use axum::body::to_bytes;
    use libsql::{Builder, params};
    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct AccountPatchProbe {
        #[serde(default)]
        account_id: WireField<i64>,
    }

    async fn test_context() -> AppContext {
        let database = Builder::new_local(":memory:").build().await.unwrap();
        AppContext::from_database(database).await.unwrap()
    }

    async fn insert_test_stream(
        connection: &Connection,
        name: &str,
        stream_type: &str,
        kind: &str,
        direction: &str,
        configuration: Option<&str>,
        parent_id: Option<i64>,
    ) -> i64 {
        let mut rows = connection
            .query(
                "INSERT INTO stream (name, type, kind, direction, amount_certainty, \
                    configuration, parent_id) \
                 VALUES (?1, ?2, ?3, ?4, 'known', ?5, ?6) RETURNING id",
                params![name, stream_type, kind, direction, configuration, parent_id],
            )
            .await
            .unwrap();
        rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap()
    }

    #[tokio::test]
    async fn canvas_page_prefers_tmo_and_renders_database_streams() {
        let context = test_context().await;
        let connection = context.connection().await.unwrap();
        let manual_id = insert_test_stream(
            &connection,
            "Manual first",
            "manual_income",
            "manual_income",
            "in",
            None,
            None,
        )
        .await;
        let trust_id = insert_test_stream(
            &connection,
            "Provider portfolio",
            "mortgage_portfolio",
            "tmo_trust",
            "in",
            None,
            None,
        )
        .await;

        let response = canvas_page(Extension(context)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains(&format!("data-default-stream-id=\"{trust_id}\"")));
        assert!(body.contains(&format!("data-stream-id=\"{manual_id}\"")));
        assert!(body.contains("Provider portfolio"));
        assert!(body.contains("Manual first"));
    }

    #[tokio::test]
    async fn canvas_storage_failure_is_a_sanitized_500() {
        let context = test_context().await;
        context
            .connection()
            .await
            .unwrap()
            .execute("ALTER TABLE stream RENAME TO unavailable_stream", ())
            .await
            .unwrap();

        let response = canvas_page(Extension(context)).await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("Canvas data could not be loaded"));
        assert!(!body.contains("no such table"));
        assert!(!body.contains("unavailable_stream"));
    }

    #[test]
    fn wire_field_distinguishes_missing_null_and_value() {
        let missing: AccountPatchProbe = serde_json::from_str("{}").unwrap();
        let null: AccountPatchProbe = serde_json::from_str(r#"{"account_id":null}"#).unwrap();
        let value: AccountPatchProbe = serde_json::from_str(r#"{"account_id":42}"#).unwrap();

        assert_eq!(missing.account_id, WireField::Missing);
        assert_eq!(null.account_id, WireField::Null);
        assert_eq!(value.account_id, WireField::Value(42));
    }

    #[test]
    fn flattened_update_round_trips_unseen_schedules_and_ids() {
        let today = "2026-07-14".parse().unwrap();
        let schedules = vec![
            StreamScheduleView {
                id: 10,
                stream_id: 7,
                account_id: Some(1),
                label: Some("Visible".into()),
                amount: 100.0,
                frequency: "monthly".into(),
                day_of_month: Some(5),
                start_date: "2026-01-01".into(),
                end_date: None,
                is_active: 1,
                metadata: None,
            },
            StreamScheduleView {
                id: 11,
                stream_id: 7,
                account_id: Some(2),
                label: Some("Invisible override".into()),
                amount: 75.0,
                frequency: "annual".into(),
                day_of_month: None,
                start_date: "2026-09-01".into(),
                end_date: Some("2030-09-01".into()),
                is_active: 1,
                metadata: Some(r#"{"kind":"override"}"#.into()),
            },
        ];

        let drafts = flattened_schedules(
            "Bills",
            &schedules,
            None,
            WireField::Value(125.0),
            WireField::Value("monthly".into()),
            WireField::Value(6),
            WireField::Value("2026-01-01".into()),
            WireField::Missing,
            Some(1),
            true,
            today,
        )
        .unwrap();

        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].id, Some(10));
        assert_eq!(drafts[0].amount, 125.0);
        assert_eq!(drafts[1].id, Some(11));
        assert_eq!(drafts[1].account_id, Some(2));
        assert_eq!(drafts[1].end_date.unwrap().to_string(), "2030-09-01");
        assert_eq!(
            drafts[1].metadata.as_deref(),
            Some(r#"{"kind":"override"}"#)
        );
    }

    #[tokio::test]
    async fn forecast_validates_unknown_filters_before_cash_onboarding() {
        let response = get_forecast(
            Extension(test_context().await),
            Query(ForecastParams {
                stream_id: Some(999),
                ..ForecastParams::default()
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["error"], "stream_not_found");
    }

    #[tokio::test]
    async fn confirmed_zero_is_a_real_forecast_anchor() {
        let context = test_context().await;
        let saved = set_cash_balance(
            Extension(context.clone()),
            Json(SetCashRequest {
                amount: 0.0,
                as_of_date: None,
            }),
        )
        .await;
        assert_eq!(saved.status(), StatusCode::OK);

        let response = get_forecast(Extension(context), Query(ForecastParams::default())).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["starting_balance"], 0.0);
        assert_eq!(payload["cash_source"]["amount"], 0.0);
    }

    #[tokio::test]
    async fn set_cash_uses_the_explicit_browser_local_date() {
        let context = test_context().await;
        let response = set_cash_balance(
            Extension(context.clone()),
            Json(SetCashRequest {
                amount: 42.0,
                as_of_date: Some("2026-07-14".into()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let connection = context.connection().await.unwrap();
        let source = FinanceRepository::new(&connection)
            .get_cash_source()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(source.amount, 42.0);
        assert_eq!(source.as_of_date, "2026-07-14");

        let response = set_cash_balance(
            Extension(context),
            Json(SetCashRequest {
                amount: 43.0,
                as_of_date: Some("07/14/2026".into()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn account_patch_omission_keeps_balance_date_and_provider_provenance() {
        let context = test_context().await;
        let connection = context.connection().await.unwrap();
        let mut rows = connection
            .query(
                "INSERT INTO account (name, kind, balance, balance_as_of_date, source_type, \
                    source_ref, balance_updated_at, is_primary) \
                 VALUES ('Imported Cash', 'checking', 1200.0, '2024-05-06', 'provider', \
                    'remote:42', '2024-05-06T12:00:00Z', 1) RETURNING id",
                (),
            )
            .await
            .unwrap();
        let id = rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap();
        drop(rows);

        let request = serde_json::from_value::<AccountRequest>(json!({
            "name": "Renamed Cash"
        }))
        .unwrap();
        let response = update_account(Extension(context), Path(id), Json(request)).await;
        assert_eq!(response.status(), StatusCode::OK);

        let mut rows = connection
            .query(
                "SELECT name, kind, balance, balance_as_of_date, source_type, source_ref, \
                        balance_updated_at, is_primary \
                 FROM account WHERE id = ?1",
                params![id],
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        assert_eq!(row.get::<String>(0).unwrap(), "Renamed Cash");
        assert_eq!(row.get::<String>(1).unwrap(), "checking");
        assert_eq!(row.get::<f64>(2).unwrap(), 1200.0);
        assert_eq!(row.get::<String>(3).unwrap(), "2024-05-06");
        assert_eq!(row.get::<String>(4).unwrap(), "provider");
        assert_eq!(row.get::<String>(5).unwrap(), "remote:42");
        assert_eq!(row.get::<String>(6).unwrap(), "2024-05-06T12:00:00Z");
        assert_eq!(row.get::<i64>(7).unwrap(), 1);
    }

    #[tokio::test]
    async fn account_patch_cannot_demote_the_current_primary() {
        let context = test_context().await;
        let connection = context.connection().await.unwrap();
        let repository = FinanceRepository::new(&connection);
        let id = repository.ensure_primary_account().await.unwrap();

        let request = serde_json::from_value::<AccountRequest>(json!({
            "name": "Primary Cash",
            "is_primary": false
        }))
        .unwrap();
        let response = update_account(Extension(context), Path(id), Json(request)).await;
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let account = repository.list_accounts().await.unwrap().remove(0);
        assert_eq!(account.id, id);
        assert_eq!(account.is_primary, 1);
    }

    #[tokio::test]
    async fn stream_patch_round_trips_hidden_imported_fields() {
        let context = test_context().await;
        let connection = context.connection().await.unwrap();
        let parent_id = insert_test_stream(
            &connection,
            "Parent",
            "provider_parent_v2",
            "tmo_trust",
            "in",
            None,
            None,
        )
        .await;
        let child_id = insert_test_stream(
            &connection,
            "Imported child",
            "provider_child_v2",
            "tmo_trust",
            "out",
            Some(r#"{"remote_id":"loan-42"}"#),
            Some(parent_id),
        )
        .await;

        let request = serde_json::from_value::<StreamRequest>(json!({
            "name": "Renamed imported child",
            "kind": "tmo_trust"
        }))
        .unwrap();
        let response = update_stream(Extension(context), Path(child_id), Json(request)).await;
        assert_eq!(response.status(), StatusCode::OK);

        let mut rows = connection
            .query(
                "SELECT name, type, kind, direction, configuration, parent_id \
                 FROM stream WHERE id = ?1",
                params![child_id],
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        assert_eq!(row.get::<String>(0).unwrap(), "Renamed imported child");
        assert_eq!(row.get::<String>(1).unwrap(), "provider_child_v2");
        assert_eq!(row.get::<String>(2).unwrap(), "tmo_trust");
        assert_eq!(row.get::<String>(3).unwrap(), "out");
        assert_eq!(row.get::<String>(4).unwrap(), r#"{"remote_id":"loan-42"}"#);
        assert_eq!(row.get::<i64>(5).unwrap(), parent_id);
    }

    #[tokio::test]
    async fn event_patch_null_account_restores_stream_default() {
        let context = test_context().await;
        let connection = context.connection().await.unwrap();
        let repository = FinanceRepository::new(&connection);
        let primary_id = repository.ensure_primary_account().await.unwrap();
        repository
            .set_starting_balance(100.0, "2026-07-14".parse().unwrap(), "manual", None, None)
            .await
            .unwrap();
        let override_id = repository
            .save_account(&AccountDraft {
                id: None,
                name: "Override".into(),
                kind: "checking".into(),
                balance: None,
                balance_as_of_date: None,
                is_primary: false,
                notes: None,
            })
            .await
            .unwrap();
        let stream_id = insert_test_stream(
            &connection,
            "Income",
            "manual_income",
            "manual_income",
            "in",
            None,
            None,
        )
        .await;
        connection
            .execute(
                "UPDATE stream SET default_account_id = ?2 WHERE id = ?1",
                params![stream_id, primary_id],
            )
            .await
            .unwrap();
        let mut rows = connection
            .query(
                "INSERT INTO stream_event (stream_id, label, expected_date, amount, status, \
                    source_id, source_type, override_account_id, has_account_override) \
                 VALUES (?1, 'Deposit', '2026-07-15', 10.0, 'projected', \
                    'manual:test', 'manual', ?2, 1) RETURNING id",
                params![stream_id, override_id],
            )
            .await
            .unwrap();
        let event_id = rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap();
        drop(rows);

        let request = serde_json::from_value::<UpdateEventRequest>(json!({
            "account_id": null
        }))
        .unwrap();
        let response = update_event(Extension(context), Path(event_id), Json(request)).await;
        assert_eq!(response.status(), StatusCode::OK);

        let forecast = repository
            .compute_forecast(ForecastQuery {
                from: "2026-07-15".parse().unwrap(),
                through: "2026-07-15".parse().unwrap(),
                today: "2026-07-14".parse().unwrap(),
                stream_id: Some(stream_id),
                view_id: None,
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(forecast.rows[0].account_id, Some(primary_id));

        let mut rows = connection
            .query(
                "SELECT override_account_id, has_account_override \
                 FROM stream_event WHERE id = ?1",
                params![event_id],
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        assert_eq!(row.get::<Option<i64>>(0).unwrap(), None);
        assert_eq!(row.get::<i64>(1).unwrap(), 0);
    }

    #[tokio::test]
    async fn view_patch_omission_preserves_membership() {
        let context = test_context().await;
        let connection = context.connection().await.unwrap();
        let repository = FinanceRepository::new(&connection);
        repository
            .bootstrap_defaults("2026-07-14".parse().unwrap())
            .await
            .unwrap();
        let before = repository.list_view_editors().await.unwrap().remove(0);
        let before_ids: Vec<_> = before
            .members
            .iter()
            .filter(|member| member.included)
            .map(|member| member.stream_id)
            .collect();
        assert!(!before_ids.is_empty());

        let request = serde_json::from_value::<ViewRequest>(json!({
            "name": before.name,
            "description": before.description
        }))
        .unwrap();
        let response = update_view(Extension(context), Path(before.id), Json(request)).await;
        assert_eq!(response.status(), StatusCode::OK);

        let after = repository.list_view_editors().await.unwrap().remove(0);
        let after_ids: Vec<_> = after
            .members
            .iter()
            .filter(|member| member.included)
            .map(|member| member.stream_id)
            .collect();
        assert_eq!(after_ids, before_ids);
    }

    #[tokio::test]
    async fn unsupported_stream_kind_is_validation_not_storage_failure() {
        let request = serde_json::from_value::<StreamRequest>(json!({
            "name": "Mystery",
            "kind": "surprise"
        }))
        .unwrap();
        let response = create_stream(Extension(test_context().await), Json(request)).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
