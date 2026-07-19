use axum::{
    Json,
    extract::{Extension, Path},
    http::StatusCode,
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
    Extension(context): Extension<AppContext>,
    Path(slug): Path<String>,
) -> Result<
    Json<http::SyncStatusResponse>,
    (StatusCode, Json<http::SyncErrorResponse>),
> {
    http::status_json(&context, &slug).await.map(Json)
}
