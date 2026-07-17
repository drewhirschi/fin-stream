use std::sync::Arc;

use axum::{Json, extract::Extension, response::Response};

use crate::{
    activity_refresh::{self, ActivityRefreshRequest},
    crypto::CredentialCipher,
    db::AppContext,
};

pub async fn post(
    context: Extension<AppContext>,
    cipher: Extension<Arc<CredentialCipher>>,
    request: Json<ActivityRefreshRequest>,
) -> Response {
    activity_refresh::post(context, cipher, request).await
}
