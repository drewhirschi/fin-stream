use axum::{
    extract::{Extension, Path},
    http::HeaderMap,
    response::Response,
};

use crate::{db::AppContext, sync_runtime::http};

pub async fn get(
    context: Extension<AppContext>,
    slug: Path<String>,
    headers: HeaderMap,
) -> Response {
    http::status_scoped(context, slug, headers).await
}
