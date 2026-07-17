use axum::{
    extract::{Extension, Path},
    response::Response,
};

use crate::{db::AppContext, workspace_inbox::http};

pub async fn get(context: Extension<AppContext>, path: Path<i64>) -> Response {
    http::email_panel(context, path).await
}
