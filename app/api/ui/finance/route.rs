use axum::{extract::Extension, response::Response};
use crate::{db::AppContext, ui};

pub async fn get(timing: nextrs::Timing, Extension(context): Extension<AppContext>) -> Response {
    ui::response(ui::finance(&context, &timing).await)
}
