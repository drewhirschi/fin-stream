use std::sync::Arc;

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use tower_sessions::Session;

use crate::AppState;

pub const SESSION_USER_ID_KEY: &str = "user_id";

/// Hash a password with Argon2id + random salt. Returns the PHC string.
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let phc = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("argon2 hash failed: {e}"))?
        .to_string();
    Ok(phc)
}

/// Verify a password against a stored Argon2 PHC string.
pub fn verify_password(password: &str, hash: &str) -> anyhow::Result<bool> {
    let parsed = PasswordHash::new(hash).map_err(|e| anyhow::anyhow!("invalid hash: {e}"))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

/// Middleware that requires a valid session with a user_id. Unauthenticated
/// requests are redirected to /login (browser) or returned 401 JSON (API/HTMX).
pub async fn require_auth(
    State(_state): State<Arc<AppState>>,
    session: Session,
    request: Request,
    next: Next,
) -> Response {
    let user_id = session.get::<i64>(SESSION_USER_ID_KEY).await.ok().flatten();

    if user_id.is_some() {
        return next.run(request).await;
    }

    let path = request.uri().path();
    let headers = request.headers();

    let is_htmx = headers
        .get("HX-Request")
        .is_some_and(|v| v.to_str().is_ok_and(|s| s.eq_ignore_ascii_case("true")));

    let is_api = path.starts_with("/api/")
        || headers
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("application/json"));

    if is_htmx {
        // HTMX expects an HX-Redirect header so it does a full-page nav,
        // not a partial swap of the login HTML.
        let mut resp = Response::new(Body::empty());
        *resp.status_mut() = StatusCode::OK;
        resp.headers_mut()
            .insert("HX-Redirect", HeaderValue::from_static("/login"));
        return resp;
    }

    if is_api {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::CONTENT_TYPE, "application/json")],
            r#"{"error":"unauthorized"}"#,
        )
            .into_response();
    }

    Redirect::to("/login").into_response()
}
