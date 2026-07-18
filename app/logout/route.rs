use axum::response::{IntoResponse, Redirect, Response};
use tower_sessions::Session;

pub async fn post(session: Session) -> Response {
    match session.flush().await {
        Ok(()) => Redirect::to("/login").into_response(),
        Err(error) => {
            tracing::error!(%error, "failed to revoke session during logout");
            crate::templates::service_unavailable_response()
        }
    }
}
