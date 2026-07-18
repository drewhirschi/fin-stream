use axum::{
    body::Body,
    extract::Extension,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
};

use crate::db::AppContext;

/// Rechecks the database dependency and all active login password hashes.
/// Schema checks also run while constructing `AppContext`; Vercel therefore
/// fails its cold start before routing if the imported schema ledger is wrong.
pub async fn get(
    Extension(context): Extension<AppContext>,
    _request: Request<Body>,
) -> Response {
    match context.ensure_usable_login().await {
        Ok(()) => (StatusCode::OK, "ready").into_response(),
        Err(error) => {
            tracing::error!(%error, "readiness check failed");
            (StatusCode::SERVICE_UNAVAILABLE, "not ready").into_response()
        }
    }
}
