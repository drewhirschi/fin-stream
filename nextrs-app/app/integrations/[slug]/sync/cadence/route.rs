use axum::{
    Form,
    extract::{Extension, Path},
    http::HeaderMap,
    response::Response,
};

use crate::{
    db::AppContext,
    integrations::actions::{self, CadenceRequest},
};

pub async fn post(
    context: Extension<AppContext>,
    slug: Path<String>,
    headers: HeaderMap,
    request: Form<CadenceRequest>,
) -> Response {
    actions::update_cadence(context, slug, headers, request).await
}
