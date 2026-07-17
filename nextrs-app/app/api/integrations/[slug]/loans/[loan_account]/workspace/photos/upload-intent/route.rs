use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path},
    response::Response,
};

use crate::{db::AppContext, media::{MediaService, http}};

pub async fn post(
    context: Extension<AppContext>,
    media: Extension<Arc<MediaService>>,
    path: Path<(String, String)>,
    request: Json<http::PhotoUploadIntentRequest>,
) -> Response {
    http::create_photo_upload_intent(context, media, path, request).await
}
