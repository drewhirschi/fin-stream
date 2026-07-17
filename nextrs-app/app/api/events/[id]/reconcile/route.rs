use axum::{
    Json,
    extract::{Extension, Path},
    response::Response,
};

use crate::{db::AppContext, finance::http};

pub async fn post(
    context: Extension<AppContext>,
    path: Path<i64>,
    request: Json<http::ReconcileRequest>,
) -> Response {
    http::reconcile_event(context, path, request).await
}
