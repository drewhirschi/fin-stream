use axum::{extract::Extension, http::HeaderMap, response::Response};
use std::sync::Arc;

use crate::{crypto::CredentialCipher, db::AppContext, sync_runtime::http};

pub async fn post(
    context: Extension<AppContext>,
    cipher: Extension<Arc<CredentialCipher>>,
    query: axum::extract::Query<http::SyncQuery>,
    headers: HeaderMap,
) -> Response {
    http::run_global(context, cipher, query, headers).await
}
