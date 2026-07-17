use axum::{extract::Extension, response::Response};
use crate::{db::AppContext, ui};

pub async fn get(Extension(context): Extension<AppContext>) -> Response {
    ui::response(ui::integrations(&context).await)
}
