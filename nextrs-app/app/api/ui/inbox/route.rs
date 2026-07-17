use axum::{extract::{Extension, Query}, response::Response};
use serde::Deserialize;
use crate::{db::AppContext, ui};

#[derive(Default, Deserialize)]
pub struct Params { #[serde(default)] show_linked: bool }

pub async fn get(Extension(context): Extension<AppContext>, Query(params): Query<Params>) -> Response {
    ui::response(ui::inbox(&context, params.show_linked).await)
}
