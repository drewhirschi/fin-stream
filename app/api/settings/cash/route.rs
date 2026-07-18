use axum::{Json, extract::Extension, response::Response};

use crate::{db::AppContext, finance::http};

pub async fn post(context: Extension<AppContext>, request: Json<http::SetCashRequest>) -> Response {
    http::set_cash_balance(context, request).await
}
