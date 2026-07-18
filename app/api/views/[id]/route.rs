use axum::{
    Json,
    extract::{Extension, Path},
    response::Response,
};

use crate::{db::AppContext, finance::http};

pub async fn patch(
    context: Extension<AppContext>,
    path: Path<i64>,
    request: Json<http::ViewRequest>,
) -> Response {
    http::update_view(context, path, request).await
}
