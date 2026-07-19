use axum::{
    extract::{Extension, Path},
    http::HeaderMap,
    response::Response,
};
use std::sync::Arc;

use crate::{crypto::CredentialCipher, db::AppContext, sync_runtime::http};

#[nextrs::api(
    post,
    operation_id = "runIntegrationSync",
    tag = "integration-sync",
    responses(
        (status = 200, description = "Sync completed", body = http::SyncExecutionResponse),
        (status = 409, description = "Sync is already running or configuration is incomplete", body = http::SyncConflictResponse),
        (status = 422, description = "Synchronization is not supported for this integration", body = http::SyncErrorResponse),
        (status = 502, description = "The provider sync failed", body = http::SyncExecutionResponse),
        (status = 503, description = "Synchronization is temporarily unavailable", body = http::SyncErrorResponse),
    ),
)]
pub async fn post(
    context: Extension<AppContext>,
    cipher: Extension<Arc<CredentialCipher>>,
    slug: Path<String>,
    headers: HeaderMap,
) -> Response {
    http::run_scoped(context, cipher, slug, headers).await
}
