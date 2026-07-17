use std::sync::Arc;

use axum::{
    extract::{Extension, Path},
    response::Response,
};

use crate::{db::AppContext, media::{MediaService, http}};

pub async fn get(
    context: Extension<AppContext>,
    media: Extension<Arc<MediaService>>,
    path: Path<String>,
) -> Response {
    http::serve_loan_photo(context, media, path).await
}
