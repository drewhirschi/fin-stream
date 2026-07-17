use std::sync::Arc;

use axum::{Json, extract::Extension, response::Response};

use crate::{
    crypto::CredentialCipher,
    db::AppContext,
    integrations::actions::{self, MonarchBalanceRequest},
};

pub async fn post(
    context: Extension<AppContext>,
    cipher: Extension<Arc<CredentialCipher>>,
    request: Json<MonarchBalanceRequest>,
) -> Response {
    actions::sync_monarch_balance(context, cipher, request).await
}
