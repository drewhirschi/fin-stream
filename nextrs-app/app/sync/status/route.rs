use axum::{extract::Extension, http::HeaderMap, response::Response};

use crate::{db::AppContext, sync_runtime::http};

pub async fn get(
    context: Extension<AppContext>,
    query: axum::extract::Query<http::SyncQuery>,
    headers: HeaderMap,
) -> Response {
    http::status_global(context, query, headers).await
}
