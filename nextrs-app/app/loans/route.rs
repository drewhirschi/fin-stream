use axum::response::Redirect;

use crate::integrations::http;

pub async fn get() -> Redirect {
    http::legacy_loans_redirect().await
}
