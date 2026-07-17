use axum::{
    extract::{Extension, Path},
    response::Response,
};

use crate::{db::AppContext, media::http};

pub async fn post(
    context: Extension<AppContext>,
    path: Path<(String, String, i64)>,
) -> Response {
    http::delete_photo(context, path).await
}
