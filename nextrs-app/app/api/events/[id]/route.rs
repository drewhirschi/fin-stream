use axum::{
    Json,
    extract::{Extension, Path},
    response::Response,
};

use crate::{db::AppContext, finance::http};

pub async fn patch(
    context: Extension<AppContext>,
    path: Path<i64>,
    request: Json<http::UpdateEventRequest>,
) -> Response {
    http::update_event(context, path, request).await
}

pub async fn delete(context: Extension<AppContext>, path: Path<i64>) -> Response {
    http::delete_event(context, path).await
}
