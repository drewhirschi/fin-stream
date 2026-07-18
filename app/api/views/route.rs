use axum::{Json, extract::Extension, response::Response};

use crate::{db::AppContext, finance::http};

pub async fn post(context: Extension<AppContext>, request: Json<http::ViewRequest>) -> Response {
    http::create_view(context, request).await
}
