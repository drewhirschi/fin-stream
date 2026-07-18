use axum::{
    Form,
    extract::Extension,
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use tower_sessions::Session;

use crate::{
    auth::{self, DUMMY_PASSWORD_HASH, SESSION_USER_ID_KEY},
    db::AppContext,
};

#[derive(Deserialize)]
pub struct LoginForm {
    email: String,
    password: String,
}

pub async fn post(
    Extension(context): Extension<AppContext>,
    session: Session,
    Form(form): Form<LoginForm>,
) -> Response {
    let email = form.email.trim().to_lowercase();
    if email.is_empty()
        || email.len() > 320
        || form.password.is_empty()
        || form.password.len() > 1_024
    {
        return invalid_login();
    }

    let user = match context.active_user_for_login(&email).await {
        Ok(user) => user,
        Err(error) => return service_unavailable(error),
    };
    let user_id = user.as_ref().map(|(user_id, _)| *user_id);
    let password_hash = user
        .map(|(_, password_hash)| password_hash)
        .unwrap_or_else(|| DUMMY_PASSWORD_HASH.to_owned());

    let password = form.password;
    let verified = match tokio::task::spawn_blocking(move || {
        auth::verify_password(&password, &password_hash)
    })
    .await
    {
        Ok(Ok(verified)) => verified,
        Ok(Err(error)) => return service_unavailable(error),
        Err(error) => return service_unavailable(error),
    };
    if !verified || user_id.is_none() {
        return invalid_login();
    }
    let user_id = user_id.expect("verified database login has a user id");

    if let Err(error) = session.cycle_id().await {
        return service_unavailable(error);
    }
    if let Err(error) = session.insert(SESSION_USER_ID_KEY, user_id).await {
        return service_unavailable(error);
    }
    if let Err(error) = session.save().await {
        return service_unavailable(error);
    }

    Redirect::to("/").into_response()
}

fn invalid_login() -> Response {
    (StatusCode::UNAUTHORIZED, "Invalid email or password.").into_response()
}

fn service_unavailable(error: impl std::fmt::Display) -> Response {
    tracing::error!(%error, "authentication backend unavailable");
    (StatusCode::SERVICE_UNAVAILABLE, "Authentication is temporarily unavailable.").into_response()
}
