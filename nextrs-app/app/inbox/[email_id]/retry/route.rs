use axum::{
    Form,
    extract::{Extension, Path},
    response::Response,
};
use std::sync::Arc;

use crate::{db::AppContext, media::MediaService, resend};

pub async fn post(
    context: Extension<AppContext>,
    service: Extension<Arc<resend::ResendService>>,
    media: Extension<Arc<MediaService>>,
    path: Path<i64>,
    form: Form<resend::http::RetryForm>,
) -> Response {
    resend::http::retry(context, service, media, path, form).await
}
