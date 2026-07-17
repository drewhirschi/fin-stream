use axum::{
    Form,
    extract::{Extension, Path},
    response::Response,
};

use crate::{db::AppContext, workspace_inbox::http};

pub async fn post(
    context: Extension<AppContext>,
    path: Path<i64>,
    form: Form<http::InboxActionForm>,
) -> Response {
    http::unlink_email(context, path, form).await
}
