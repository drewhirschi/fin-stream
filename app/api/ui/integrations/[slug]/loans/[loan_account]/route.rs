use axum::{extract::{Extension, Path}, response::Response};
use crate::{db::AppContext, ui};

pub async fn get(timing: nextrs::Timing, Extension(context): Extension<AppContext>, Path((slug, loan_account)): Path<(String, String)>) -> Response {
    ui::optional_response(timing.span("db", ui::loan(&context, &slug, &loan_account)).await)
}
