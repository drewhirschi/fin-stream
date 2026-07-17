use axum::{
    extract::{Extension, Path},
    response::Response,
};

use crate::{db::AppContext, media::http};

pub async fn post(
    context: Extension<AppContext>,
    path: Path<(String, String, i64)>,
) -> Response {
    http::feature_photo(context, path).await
}
