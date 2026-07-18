use axum::{
    Form,
    extract::{Extension, Path},
    response::Response,
};

use crate::{db::AppContext, media::http};

pub async fn post(
    context: Extension<AppContext>,
    path: Path<(String, String)>,
    form: Form<http::WorkspaceForm>,
) -> Response {
    http::save_workspace(context, path, form).await
}
