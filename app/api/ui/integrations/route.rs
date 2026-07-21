use axum::{extract::Extension, response::Response};
use crate::{db::AppContext, ui};

pub async fn get(timing: nextrs::Timing, Extension(context): Extension<AppContext>) -> Response {
    ui::response(timing.span("db", ui::integrations(&context)).await)
}
