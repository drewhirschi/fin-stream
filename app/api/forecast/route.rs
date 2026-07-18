use axum::{
    extract::{Extension, Query},
    response::Response,
};

use crate::{db::AppContext, finance::http};

pub async fn get(context: Extension<AppContext>, query: Query<http::ForecastParams>) -> Response {
    http::get_forecast(context, query).await
}
