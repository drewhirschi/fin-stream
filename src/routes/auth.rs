use std::sync::Arc;

use axum::{
    Form, Router,
    extract::State,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use tower_sessions::Session;

use crate::{AppState, auth, db, templates::LoginTemplate};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/login", get(login_page).post(login_submit))
        .route("/logout", post(logout))
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub email: String,
    pub password: String,
}

async fn login_page(session: Session) -> Response {
    if let Ok(Some(_)) = session.get::<i64>(auth::SESSION_USER_ID_KEY).await {
        return Redirect::to("/").into_response();
    }
    LoginTemplate {
        title: "Sign in".into(),
        error: None,
    }
    .into_response()
}

async fn login_submit(
    State(state): State<Arc<AppState>>,
    session: Session,
    Form(form): Form<LoginForm>,
) -> Response {
    let email = form.email.trim().to_lowercase();
    if email.is_empty() || form.password.is_empty() {
        return LoginTemplate {
            title: "Sign in".into(),
            error: Some("Email and password are required.".into()),
        }
        .into_response();
    }

    let user = match db::users::get_user_by_email(&state.db, &email).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return render_invalid();
        }
        Err(e) => {
            tracing::error!("user lookup failed: {e}");
            return LoginTemplate {
                title: "Sign in".into(),
                error: Some("Something went wrong. Try again.".into()),
            }
            .into_response();
        }
    };

    let (user_id, _user_email, password_hash) = user;

    // Argon2 is intentionally slow — run off the async runtime.
    let password = form.password;
    let hash = password_hash.clone();
    let verify_result =
        tokio::task::spawn_blocking(move || auth::verify_password(&password, &hash)).await;

    let ok = matches!(verify_result, Ok(Ok(true)));

    if !ok {
        return render_invalid();
    }

    if let Err(e) = session.insert(auth::SESSION_USER_ID_KEY, user_id).await {
        tracing::error!("failed to write session: {e}");
        return LoginTemplate {
            title: "Sign in".into(),
            error: Some("Could not start session. Try again.".into()),
        }
        .into_response();
    }

    Redirect::to("/").into_response()
}

async fn logout(session: Session) -> Response {
    if let Err(e) = session.flush().await {
        tracing::error!("failed to flush session on logout: {e}");
    }
    Redirect::to("/login").into_response()
}

fn render_invalid() -> Response {
    LoginTemplate {
        title: "Sign in".into(),
        error: Some("Invalid email or password.".into()),
    }
    .into_response()
}
