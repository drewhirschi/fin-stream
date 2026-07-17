use axum::{
    Json,
    extract::{Extension, Path},
    response::Response,
};

use crate::{db::AppContext, finance::http};

pub async fn patch(
    context: Extension<AppContext>,
    path: Path<i64>,
    request: Json<http::AccountRequest>,
) -> Response {
    http::update_account(context, path, request).await
}
