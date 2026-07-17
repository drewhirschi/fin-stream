use axum::{
    Json,
    extract::{Extension, Request},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::{
    db::AppContext,
    operations::{OperationError, OperationRepository},
};

const RETRY_AFTER_SECONDS: &str = "60";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GateRequirement {
    Exempt,
    Writes,
    Scheduler,
}

/// Classify before route matching so a newly added mutation is protected even
/// if its handler forgets to opt in. Login/logout are the only session
/// mutations and deliberately remain available during a read-only soak.
pub(crate) fn requirement(method: &Method, path: &str) -> GateRequirement {
    if path == "/login" || path == "/logout" {
        return GateRequirement::Exempt;
    }
    if path == "/internal/cron" || path.starts_with("/internal/cron/") {
        return GateRequirement::Scheduler;
    }
    if matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) {
        GateRequirement::Writes
    } else {
        GateRequirement::Exempt
    }
}

pub(crate) async fn enforce(
    Extension(context): Extension<AppContext>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_owned();
    let requirement = requirement(request.method(), &path);
    if requirement == GateRequirement::Exempt {
        return next.run(request).await;
    }

    let connection = match context.connection().await {
        Ok(connection) => connection,
        Err(error) => {
            tracing::error!(%error, "write gate could not open operation-control connection");
            return blocked_response(&path, GateFailure::Unavailable);
        }
    };
    let repository = OperationRepository::new(&connection);
    let result = match requirement {
        GateRequirement::Exempt => unreachable!("exempt requests return before database access"),
        GateRequirement::Writes => repository.require_writes_enabled().await,
        GateRequirement::Scheduler => repository.require_scheduler_enabled().await,
    };
    match result {
        Ok(_) => next.run(request).await,
        Err(OperationError::ReadOnly) => blocked_response(&path, GateFailure::ReadOnly),
        Err(OperationError::SchedulerDisabled) => {
            blocked_response(&path, GateFailure::SchedulerDisabled)
        }
        Err(error) => {
            tracing::error!(%error, "write gate could not read operation control");
            blocked_response(&path, GateFailure::Unavailable)
        }
    }
}

#[derive(Clone, Copy)]
enum GateFailure {
    ReadOnly,
    SchedulerDisabled,
    Unavailable,
}

impl GateFailure {
    const fn code(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::SchedulerDisabled => "scheduler_disabled",
            Self::Unavailable => "service_unavailable",
        }
    }

    const fn message(self) -> &'static str {
        match self {
            Self::ReadOnly => "Writes are temporarily disabled.",
            Self::SchedulerDisabled => "Scheduled work is temporarily disabled.",
            Self::Unavailable => "Operation control is temporarily unavailable.",
        }
    }
}

fn blocked_response(path: &str, failure: GateFailure) -> Response {
    let mut response = if path == "/api"
        || path.starts_with("/api/")
        || path == "/internal/cron"
        || path.starts_with("/internal/cron/")
        || crate::resend::http::is_webhook_path(path)
    {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": failure.code(),
                "message": failure.message(),
            })),
        )
            .into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, failure.message()).into_response()
    };
    response.headers_mut().insert(
        header::RETRY_AFTER,
        HeaderValue::from_static(RETRY_AFTER_SECONDS),
    );
    response
}
