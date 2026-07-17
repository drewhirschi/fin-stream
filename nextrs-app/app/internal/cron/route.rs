use axum::{
    extract::Extension,
    response::Response,
};
use std::sync::Arc;

use crate::{crypto::CredentialCipher, db::AppContext, scheduler};

pub async fn get(
    context: Extension<AppContext>,
    cipher: Extension<Arc<CredentialCipher>>,
) -> Response {
    scheduler::run_cron(context, cipher).await
}
