use axum::{body::Bytes, extract::Extension, response::Response};
use std::sync::Arc;

use crate::{db::AppContext, media::MediaService, resend};

pub async fn post(
    context: Extension<AppContext>,
    service: Extension<Arc<resend::ResendService>>,
    media: Extension<Arc<MediaService>>,
    body: Bytes,
) -> Response {
    resend::http::webhook(context, service, media, body).await
}
