use axum::{
    extract::{Extension, Path},
    http::HeaderMap,
    response::Response,
};

use crate::{db::AppContext, sync_runtime::http};

#[nextrs::api(
    get,
    operation_id = "getIntegrationSyncStatus",
    tag = "integration-sync",
    responses(
        (status = 200, description = "Current or most recent sync execution", body = http::SyncStatusResponse),
        (status = 422, description = "Synchronization is not supported for this integration", body = http::SyncErrorResponse),
        (status = 503, description = "Sync status is temporarily unavailable", body = http::SyncErrorResponse),
    ),
)]
pub async fn get(
    context: Extension<AppContext>,
    slug: Path<String>,
    headers: HeaderMap,
) -> Response {
    http::status_scoped(context, slug, headers).await
}
