use axum::{extract::{Extension, Path}, response::Response};
use crate::{db::AppContext, ui};

pub async fn get(Extension(context): Extension<AppContext>, Path((slug, loan_account)): Path<(String, String)>) -> Response {
    ui::optional_response(ui::loan(&context, &slug, &loan_account).await)
}
