use axum::{Json, extract::Extension, response::Response};

use crate::{db::AppContext, finance::http};

pub async fn post(context: Extension<AppContext>, request: Json<http::AccountRequest>) -> Response {
    http::create_account(context, request).await
}
