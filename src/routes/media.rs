use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};

use crate::{AppState, media_storage::MediaStorage};

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/media/loan-workspace/{*key}", get(loan_workspace_media))
}

async fn loan_workspace_media(
    State(_state): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> Response {
    let storage = match MediaStorage::from_env().await {
        Ok(storage) => storage,
        Err(error) => {
            tracing::error!("failed to initialize media storage: {}", error);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let Some(media) = (match storage.get(&key).await {
        Ok(media) => media,
        Err(error) => {
            tracing::error!("failed to read media object {}: {}", key, error);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let mut response = Response::new(Body::from(media.bytes));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(
            media
                .content_type
                .as_deref()
                .unwrap_or("application/octet-stream"),
        )
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );

    response
}
