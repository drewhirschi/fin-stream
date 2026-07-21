use axum::{extract::{Extension, Path}, response::Response};
use crate::{db::AppContext, ui};

pub async fn get(timing: nextrs::Timing, Extension(context): Extension<AppContext>, Path(email_id): Path<i64>) -> Response {
    ui::optional_response(timing.span("db", ui::email(&context, email_id)).await)
}
