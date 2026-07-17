use axum::{
    extract::{Extension, Path},
    http::HeaderMap,
    response::Response,
};
use std::sync::Arc;

use crate::{crypto::CredentialCipher, db::AppContext, sync_runtime::http};

pub async fn post(
    context: Extension<AppContext>,
    cipher: Extension<Arc<CredentialCipher>>,
    slug: Path<String>,
    headers: HeaderMap,
) -> Response {
    http::run_scoped(context, cipher, slug, headers).await
}
