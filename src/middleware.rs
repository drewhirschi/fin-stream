use argon2::password_hash::rand_core::{OsRng, RngCore};
use axum::{
    Json,
    body::Body,
    extract::{Extension, Request, State},
    http::{HeaderValue, Method, StatusCode, Uri, header},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use serde_json::json;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tower_sessions::Session;

use crate::{
    auth::SESSION_USER_ID_KEY, cron_auth::CronAuthenticator, crypto::CredentialCipher,
    db::AppContext, templates,
};

const REQUEST_ID_HEADER: &str = "x-request-id";
const CONTENT_SECURITY_POLICY_START: &str =
    "default-src 'self'; base-uri 'self'; connect-src 'self'";
const CONTENT_SECURITY_POLICY_MEDIA_SEPARATOR: &str =
    "; font-src 'self'; frame-ancestors 'none'; img-src 'self' data:";
const CONTENT_SECURITY_POLICY_END: &str = "; object-src 'none'; form-action 'self'; script-src 'self' 'unsafe-inline' 'unsafe-eval'; style-src 'self' 'unsafe-inline'";

#[derive(Clone, Default)]
struct AppTiming(Arc<Mutex<Vec<(&'static str, Duration)>>>);

impl AppTiming {
    fn mark(&self, name: &'static str, duration: Duration) {
        if let Ok(mut segments) = self.0.lock() {
            segments.push((name, duration));
        }
    }

    fn value(&self) -> String {
        self.0
            .lock()
            .map(|segments| {
                segments
                    .iter()
                    .map(|(name, duration)| {
                        format!("{name};dur={:.1}", duration.as_secs_f64() * 1_000.0)
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    }
}

pub async fn record_app_timing(mut request: Request, next: Next) -> Response {
    let timing = AppTiming::default();
    request.extensions_mut().insert(timing.clone());
    let mut response = next.run(request).await;
    let app_value = timing.value();
    if app_value.is_empty() {
        return response;
    }
    let combined = response
        .headers()
        .get("server-timing")
        .and_then(|value| value.to_str().ok())
        .map_or_else(
            || app_value.clone(),
            |value| format!("{value}, {app_value}"),
        );
    if let Ok(value) = HeaderValue::from_str(&combined) {
        response.headers_mut().insert("server-timing", value);
    }
    response
}

#[derive(Clone, Debug)]
pub(crate) struct ResponsePerimeterConfig {
    cookie_secure: bool,
    content_security_policy: HeaderValue,
}

impl ResponsePerimeterConfig {
    pub(crate) fn new(cookie_secure: bool, media_origin: Option<&str>) -> Self {
        let media_source = media_origin.map_or(String::new(), |origin| format!(" {origin}"));
        let policy = format!(
            "{CONTENT_SECURITY_POLICY_START}{media_source}{CONTENT_SECURITY_POLICY_MEDIA_SEPARATOR}{media_source}{CONTENT_SECURITY_POLICY_END}"
        );
        Self {
            cookie_secure,
            content_security_policy: HeaderValue::from_str(&policy)
                .expect("validated object-storage origins produce a valid CSP"),
        }
    }
}

/// Apply the HTTP perimeter after every inner layer has produced a response.
///
/// The current templates use inline styles/scripts and Alpine's expression
/// evaluator, so the CSP preserves those capabilities while still closing
/// unrelated resource classes and framing.
pub async fn response_perimeter(
    State(config): State<ResponsePerimeterConfig>,
    mut request: Request,
    next: Next,
) -> Response {
    let is_static = is_static(request.uri().path());
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .filter(|value| valid_request_id(value))
        .cloned()
        .unwrap_or_else(generated_request_id);
    request
        .headers_mut()
        .insert(REQUEST_ID_HEADER, request_id.clone());

    let mut response = next.run(request).await;
    let response_is_success = response.status().is_success();
    let headers = response.headers_mut();
    headers.insert(REQUEST_ID_HEADER, request_id);
    headers.insert("content-security-policy", config.content_security_policy);
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), geolocation=(), microphone=(), payment=()"),
    );
    headers.insert(
        "cross-origin-opener-policy",
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("same-origin"),
    );
    if config.cookie_secure {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }
    if is_static && response_is_success && !headers.contains_key(header::CACHE_CONTROL) {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=3600, must-revalidate"),
        );
    } else if !headers.contains_key(header::CACHE_CONTROL) {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, no-store"),
        );
    }
    response
}

pub async fn require_auth(
    wait: nextrs::WaitUntil,
    Extension(context): Extension<AppContext>,
    Extension(cron_authenticator): Extension<CronAuthenticator>,
    Extension(cipher): Extension<Arc<CredentialCipher>>,
    session: Session,
    request: Request,
    next: Next,
) -> Response {
    let app_timing = request.extensions().get::<AppTiming>().cloned();
    let path = request.uri().path();
    if crate::resend::http::is_webhook_path(path) {
        if request
            .extensions()
            .get::<crate::resend::http::VerifiedResendWebhook>()
            .is_none()
        {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "invalid_signature" })),
            )
                .into_response();
        }
        return next.run(request).await;
    }
    if is_cron(path) {
        if !cron_authenticator.authorizes(request.headers()) {
            return cron_unauthenticated_response();
        }
        // Axum GET routes may also match HEAD. This endpoint is deliberately
        // mutating, so only an explicit GET may pass to the durable gate.
        if request.method() != Method::GET {
            return cron_method_not_allowed_response();
        }
        return next.run(request).await;
    }

    if is_mutation(request.method()) && !has_same_origin(request.headers()) {
        return (StatusCode::FORBIDDEN, "Cross-origin mutation rejected.").into_response();
    }

    if is_public(path) {
        return next.run(request).await;
    }

    let started = Instant::now();
    let user_id_result = session.get::<i64>(SESSION_USER_ID_KEY).await;
    if let Some(timing) = &app_timing {
        timing.mark("session", started.elapsed());
    }
    let user_id = match user_id_result {
        Ok(user_id) => user_id,
        Err(error) => {
            tracing::error!(%error, "failed to load session");
            return templates::service_unavailable_response();
        }
    };
    if let Some(user_id) = user_id {
        let started = Instant::now();
        let active_result = context.user_is_active(user_id).await;
        if let Some(timing) = &app_timing {
            timing.mark("auth-db", started.elapsed());
        }
        match active_result {
            Ok(true) => {
                // Requires the vendored `vercel_runtime` end-message drain
                // (vendor/, upstream PR vercel/vercel#17350): stock 2.4.0
                // suspends the instance after the response and this background
                // work starves until its deadline or ownership lease expires.
                if cfg!(not(test)) && triggers_activity_refresh(&request) {
                    crate::activity_refresh::schedule_tmo_if_stale(&wait, context.clone(), cipher);
                }
                return next.run(request).await;
            }
            Ok(false) => {
                if let Err(error) = session.flush().await {
                    tracing::error!(%error, "failed to revoke inactive user's session");
                    return templates::service_unavailable_response();
                }
            }
            Err(error) => {
                tracing::error!(%error, "failed to validate session user");
                return templates::service_unavailable_response();
            }
        }
    }

    unauthenticated_response(path, request.headers())
}

fn triggers_activity_refresh(request: &Request) -> bool {
    if request.method() != Method::GET {
        return false;
    }

    // NextRS client navigation can reach the server either as a document load
    // or as the authenticated UI-data request that hydrates a client route.
    // Status polling and other APIs are deliberately excluded so they cannot
    // continuously schedule redundant due checks.
    if request.uri().path().starts_with("/api/ui/") {
        return true;
    }

    let headers = request.headers();
    let fetch_navigation = headers
        .get("sec-fetch-mode")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("navigate"))
        && headers
            .get("sec-fetch-dest")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.eq_ignore_ascii_case("document"));
    let accepts_html = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .filter_map(|media_type| media_type.split(';').next())
                .any(|media_type| media_type.trim().eq_ignore_ascii_case("text/html"))
        });

    fetch_navigation || accepts_html
}

fn is_mutation(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

fn has_same_origin(headers: &axum::http::HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        // Browsers send Origin on form and fetch mutations. Non-browser tools
        // may omit it, so the absence alone is not treated as cross-origin.
        return true;
    };
    let (Ok(origin), Some(host)) = (origin.to_str(), headers.get(header::HOST)) else {
        return false;
    };
    let (Ok(origin), Ok(host)) = (origin.parse::<Uri>(), host.to_str()) else {
        return false;
    };
    if origin
        .authority()
        .is_some_and(|authority| authority.as_str().eq_ignore_ascii_case(host))
    {
        return true;
    }

    // Managed serverless proxies may replace Host with an internal routing
    // authority after the browser has already computed Origin. Fetch Metadata
    // is supplied by browsers and cannot be set by page JavaScript; accepting
    // only the exact `same-origin` value preserves the CSRF boundary without
    // trusting user-controlled forwarding headers.
    headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("same-origin"))
}

fn is_public(path: &str) -> bool {
    path == "/login"
        || path == "/health"
        || path == "/healthz"
        || path == "/ready"
        // TEMPORARY (2026-08-01): log-only wait_until diagnostic probe;
        // remove together with app/internal/waituntil-probe-20260801/.
        || path == "/internal/waituntil-probe-20260801"
        || is_static(path)
}

fn is_cron(path: &str) -> bool {
    path == "/internal/cron" || path.starts_with("/internal/cron/")
}

fn is_static(path: &str) -> bool {
    path == "/static"
        || path.starts_with("/static/")
        || path == "/dist"
        || path.starts_with("/dist/")
        || path == "/style.css"
}

fn unauthenticated_response(path: &str, headers: &axum::http::HeaderMap) -> Response {
    let is_htmx = headers
        .get("HX-Request")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("true"));
    if is_htmx {
        let mut response = Response::new(Body::empty());
        response
            .headers_mut()
            .insert("HX-Redirect", HeaderValue::from_static("/login"));
        return response;
    }
    if wants_json(path, headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    Redirect::to("/login").into_response()
}

fn cron_unauthenticated_response() -> Response {
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "unauthorized" })),
    )
        .into_response();
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Bearer realm=\"cron\""),
    );
    response
}

fn cron_method_not_allowed_response() -> Response {
    let mut response = (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(json!({ "error": "method_not_allowed" })),
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static("GET"));
    response
}

pub async fn not_found(request: Request) -> Response {
    let path = request.uri().path();
    if is_static(path) {
        return (StatusCode::NOT_FOUND, "Not found.").into_response();
    }
    if wants_json(path, request.headers()) {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response();
    }
    templates::not_found_response()
}

fn wants_json(path: &str, headers: &axum::http::HeaderMap) -> bool {
    path == "/api"
        || path.starts_with("/api/")
        || crate::resend::http::is_webhook_path(path)
        || headers
            .get(header::ACCEPT)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("application/json"))
}

fn valid_request_id(value: &HeaderValue) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.'))
}

fn generated_request_id() -> HeaderValue {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut random = [0_u8; 16];
    OsRng.fill_bytes(&mut random);
    let mut value = String::with_capacity(35);
    value.push_str("td_");
    for byte in random {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    HeaderValue::from_str(&value).expect("generated request ids contain only visible ASCII")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: Method, accept: Option<&str>) -> Request {
        let mut builder = Request::builder().method(method).uri("/integrations/tmo");
        if let Some(accept) = accept {
            builder = builder.header(header::ACCEPT, accept);
        }
        builder.body(Body::empty()).unwrap()
    }

    #[test]
    fn background_refresh_follows_page_and_ui_data_activity() {
        assert!(triggers_activity_refresh(&request(
            Method::GET,
            Some("text/html,application/xhtml+xml")
        )));
        assert!(!triggers_activity_refresh(&request(
            Method::GET,
            Some("application/json")
        )));
        assert!(!triggers_activity_refresh(&request(
            Method::POST,
            Some("text/html")
        )));

        let ui_data = Request::builder()
            .method(Method::GET)
            .uri("/api/ui/integrations/tmo")
            .header(header::ACCEPT, "application/json")
            .body(Body::empty())
            .unwrap();
        assert!(triggers_activity_refresh(&ui_data));

        let fetch_navigation = Request::builder()
            .method(Method::GET)
            .uri("/integrations/tmo/sync")
            .header("sec-fetch-mode", "navigate")
            .header("sec-fetch-dest", "document")
            .body(Body::empty())
            .unwrap();
        assert!(triggers_activity_refresh(&fetch_navigation));
    }

    #[test]
    fn request_id_validation_is_narrow_and_bounded() {
        assert!(valid_request_id(&HeaderValue::from_static(
            "client-123_ABC.xyz"
        )));
        assert!(!valid_request_id(&HeaderValue::from_static(
            "contains spaces"
        )));
        assert!(!valid_request_id(&HeaderValue::from_static(
            "contains/slash"
        )));
        assert!(!valid_request_id(
            &HeaderValue::from_str(&"a".repeat(65)).unwrap()
        ));
    }

    #[test]
    fn generated_request_ids_are_safe() {
        let value = generated_request_id();
        assert!(valid_request_id(&value));
        assert!(value.to_str().unwrap().starts_with("td_"));
    }

    #[test]
    fn media_csp_allows_only_the_validated_object_origin() {
        let config =
            ResponsePerimeterConfig::new(true, Some("https://objects.example.invalid:8443"));
        let policy = config.content_security_policy.to_str().unwrap();
        assert!(policy.contains("connect-src 'self' https://objects.example.invalid:8443;"));
        assert!(policy.contains("img-src 'self' data: https://objects.example.invalid:8443;"));
        assert!(!policy.contains("img-src *"));
        assert!(!policy.contains("https:;"));
    }
}
