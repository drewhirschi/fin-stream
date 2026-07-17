use axum::{
    Json,
    extract::{Extension, Path},
    response::Response,
};

use crate::{db::AppContext, finance::http};

pub async fn patch(
    context: Extension<AppContext>,
    path: Path<i64>,
    request: Json<http::StreamRequest>,
) -> Response {
    http::update_stream(context, path, request).await
}

pub async fn delete(context: Extension<AppContext>, path: Path<i64>) -> Response {
    http::delete_stream(context, path).await
}
