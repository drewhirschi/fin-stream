use axum::{extract::{Extension, Path, Query}, response::Response};
use serde::Deserialize;
use crate::{db::AppContext, ui};

#[derive(Deserialize)]
pub struct Params { section: Option<String> }

pub async fn get(Extension(context): Extension<AppContext>, Path(slug): Path<String>, Query(params): Query<Params>) -> Response {
    ui::optional_response(ui::integration(&context, &slug, params.section.as_deref().unwrap_or("overview")).await)
}
