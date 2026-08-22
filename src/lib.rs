#![allow(clippy::match_single_binding)] // Generated NextRS asset registry has an empty loading match.

pub mod activity_refresh;
mod auth;
mod config;
mod cron_auth;
pub mod crypto;
mod db;
mod filters;
pub mod finance;
pub mod integrations;
pub mod media;
mod middleware;
pub mod operations;
mod operator;
pub mod providers;
pub mod resend;
pub mod scheduler;
mod session_store;
pub mod sync_runtime;
mod templates;
mod ui;
pub mod workspace_inbox;
mod write_gate;

pub use operator::{OperationCommand, execute_operation_command_from_env};

use axum::{
    Router,
    extract::{DefaultBodyLimit, Extension},
    middleware::{from_fn, from_fn_with_state},
};
use session_store::LibsqlSessionStore;
use std::sync::Arc;
use time::Duration;
#[cfg(feature = "local-server")]
use tower_http::services::{ServeDir, ServeFile};
use tower_sessions::{Expiry, SessionManagerLayer, SessionStore, cookie::SameSite};

// build.rs generates this registry from app/. Keeping it in the library makes
// the local server and Vercel function consume exactly the same route graph.
include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

/// Install one process-wide, secret-safe application log sink. Repeated calls
/// are harmless, which keeps the local and Vercel entry points symmetrical.
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .try_init();
}

pub async fn configured_router() -> anyhow::Result<Router> {
    let config = config::AppConfig::from_env()?;
    let credential_cipher = Arc::new(crypto::CredentialCipher::new(
        config.app_encryption_key.expose_secret(),
    )?);
    let media_service = Arc::new(media::MediaService::from_env()?);
    let resend_service = Arc::new(resend::ResendService::from_env()?);
    let context = db::AppContext::connect(&config).await?;
    context.ensure_usable_login().await?;
    let store = LibsqlSessionStore::new(context.clone());
    Ok(router_with_store(
        context,
        store,
        config.cookie_secure,
        credential_cipher,
        config.cron_authenticator,
        media_service,
        resend_service,
    ))
}

/// Explicit local operator bootstrap. Application startup and Vercel cold
/// starts never create users or domain defaults as a side effect.
#[cfg(feature = "local-db")]
pub async fn bootstrap_local_from_env() -> anyhow::Result<finance::BootstrapResult> {
    let config = config::AppConfig::from_env()?;
    let _credential_cipher =
        crypto::CredentialCipher::new(config.app_encryption_key.expose_secret())?;
    let context = db::AppContext::connect(&config).await?;
    bootstrap_local_context(
        &context,
        config.admin_email.as_deref(),
        config.admin_password.as_deref(),
    )
    .await
}

#[cfg(feature = "local-db")]
async fn bootstrap_local_context(
    context: &db::AppContext,
    admin_email: Option<&str>,
    admin_password: Option<&str>,
) -> anyhow::Result<finance::BootstrapResult> {
    context.bootstrap_admin(admin_email, admin_password).await?;
    context.ensure_usable_login().await?;

    let now = time::OffsetDateTime::now_utc().date();
    let today = finance::IsoDate::new(now.year(), now.month() as u8, now.day())?;
    let connection = context.connection().await?;
    let result = finance::FinanceRepository::new(&connection)
        .bootstrap_defaults(today)
        .await
        .map_err(anyhow::Error::from)?;
    operations::OperationRepository::new(&connection)
        .enable_writes(&operations::utc_now_millis())
        .await?;
    Ok(result)
}

fn router_with_store<S>(
    context: db::AppContext,
    store: S,
    cookie_secure: bool,
    credential_cipher: Arc<crypto::CredentialCipher>,
    cron_authenticator: cron_auth::CronAuthenticator,
    media_service: Arc<media::MediaService>,
    resend_service: Arc<resend::ResendService>,
) -> Router
where
    S: SessionStore + Clone,
{
    let perimeter_config = middleware::ResponsePerimeterConfig::new(
        cookie_secure,
        media_service.content_security_origin(),
    );
    let session_layer = SessionManagerLayer::new(store)
        .with_name("__td_session")
        .with_http_only(true)
        .with_same_site(SameSite::Strict)
        .with_secure(cookie_secure)
        .with_path("/")
        .with_expiry(Expiry::OnInactivity(Duration::days(7)));
    let router = nextrs::router::build_router_with_speculation(
        generated_registry(),
        nextrs::SpeculationConfig::OFF,
    );
    #[cfg(feature = "local-server")]
    let router = router.nest_service(
        "/static",
        ServeDir::new(concat!(env!("CARGO_MANIFEST_DIR"), "/public/static")),
    );
    #[cfg(feature = "local-server")]
    let router = router
        .nest_service(
            "/dist",
            ServeDir::new(concat!(env!("CARGO_MANIFEST_DIR"), "/public/dist")),
        )
        .route_service(
            "/style.css",
            ServeFile::new(concat!(env!("CARGO_MANIFEST_DIR"), "/public/style.css")),
        );
    router
        .fallback(middleware::not_found)
        // Bound extractor buffering, including public login requests. Current
        // JSON/form contracts are intentionally much smaller than this.
        .layer(DefaultBodyLimit::max(64 * 1_024))
        // Axum layers execute bottom-up: sessions and auth/origin checks run
        // before the default-deny business-data write gate.
        .layer(from_fn(write_gate::enforce))
        .layer(from_fn(middleware::require_auth))
        .layer(session_layer)
        .layer(from_fn(resend::http::authenticate_webhook))
        .layer(Extension(resend_service))
        .layer(Extension(media_service))
        .layer(Extension(cron_authenticator))
        .layer(Extension(credential_cipher))
        .layer(Extension(context))
        // This is deliberately outermost so redirects, extractor rejections,
        // route fallbacks, and local static responses share one perimeter.
        .layer(from_fn_with_state(
            perimeter_config,
            middleware::response_perimeter,
        ))
        // Outermost application timing captures session/auth work that runs
        // before NextRS's generated route middleware.
        .layer(from_fn(middleware::record_app_timing))
}

#[cfg(all(feature = "remote-db", feature = "local-db"))]
compile_error!(
    "local-db and remote-db are mutually exclusive; use --no-default-features --features remote-db for Vercel"
);

#[cfg(not(any(feature = "local-db", feature = "remote-db")))]
compile_error!("enable exactly one database feature: local-db or remote-db");

#[cfg(all(test, feature = "local-db"))]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use libsql::Builder;
    use time::{Duration, OffsetDateTime};
    use tower::ServiceExt;
    use tower_sessions::{
        SessionStore,
        session::{Id, Record},
        session_store,
    };

    use super::{
        bootstrap_local_context,
        cron_auth::CronAuthenticator,
        crypto::CredentialCipher,
        db::AppContext,
        generated_openapi, generated_registry,
        media::MediaService,
        operations::{OperationMode, OperationRepository},
        resend::ResendService,
        router_with_store,
        session_store::LibsqlSessionStore,
        write_gate::{GateRequirement, requirement},
    };

    const EMAIL: &str = "admin@example.com";
    const PASSWORD: &str = "correct horse battery staple";

    async fn test_context(seed_admin: bool) -> AppContext {
        let database = Builder::new_local(":memory:").build().await.unwrap();
        let context = AppContext::from_database(database).await.unwrap();
        if seed_admin {
            context
                .bootstrap_admin(Some(EMAIL), Some(PASSWORD))
                .await
                .unwrap();
        }
        context
    }

    async fn test_app(seed_admin: bool) -> axum::Router {
        test_app_with_context(seed_admin).await.0
    }

    async fn test_app_with_context(seed_admin: bool) -> (axum::Router, AppContext) {
        let context = test_context(seed_admin).await;
        let store = LibsqlSessionStore::new(context.clone());
        (
            router_with_store(
                context.clone(),
                store,
                false,
                Arc::new(CredentialCipher::new("test-router-key").unwrap()),
                CronAuthenticator::new(Some("test-cron-secret")),
                Arc::new(MediaService::disabled()),
                Arc::new(ResendService::disabled()),
            ),
            context,
        )
    }

    fn login_request(password: &str) -> Request<Body> {
        Request::post("/login")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "email=admin%40example.com&password={}",
                password.replace(' ', "+")
            )))
            .unwrap()
    }

    fn cron_request(secret: Option<&str>) -> Request<Body> {
        let mut request = Request::get("/internal/cron");
        if let Some(secret) = secret {
            request = request.header(header::AUTHORIZATION, format!("Bearer {secret}"));
        }
        request.body(Body::empty()).unwrap()
    }

    fn session_cookie(response: &axum::response::Response) -> String {
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| value.starts_with("__td_session="))
            .and_then(|value| value.split(';').next())
            .expect("session cookie")
            .to_owned()
    }

    fn assert_security_headers(response: &axum::response::Response) {
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(response.headers()["x-frame-options"], "DENY");
        assert_eq!(response.headers()["referrer-policy"], "no-referrer");
        assert_eq!(
            response.headers()["cross-origin-opener-policy"],
            "same-origin"
        );
        assert!(response.headers().contains_key("content-security-policy"));
        assert!(response.headers().contains_key("permissions-policy"));
        assert!(response.headers().contains_key("x-request-id"));
    }

    #[tokio::test]
    async fn anonymous_root_redirects_to_login() {
        let response = test_app(false)
            .await
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[header::LOCATION], "/login");
    }

    #[tokio::test]
    async fn login_page_is_public() {
        let response = test_app(false)
            .await
            .oneshot(Request::get("/login").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("__nx_root__"));
        assert!(body.contains("/dist/"));
    }

    #[tokio::test]
    async fn startup_requires_an_active_login_user() {
        let empty = test_context(false).await;
        assert!(empty.ensure_usable_login().await.is_err());

        let seeded = test_context(true).await;
        seeded.ensure_usable_login().await.unwrap();
    }

    #[tokio::test]
    async fn invalid_login_is_generic_and_sets_no_cookie() {
        let response = test_app(true)
            .await
            .oneshot(login_request("wrong password"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().get(header::SET_COOKIE).is_none());
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("Invalid email or password."));
    }

    #[tokio::test]
    async fn valid_login_sets_cookie_and_opens_dashboard() {
        let app = test_app(true).await;
        let response = app.clone().oneshot(login_request(PASSWORD)).await.unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let cookie = session_cookie(&response);
        let set_cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Strict"));

        let dashboard = app
            .oneshot(
                Request::get("/")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(dashboard.status(), StatusCode::OK);
        let body = to_bytes(dashboard.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("__nx_root__"));
        assert!(body.contains("/dist/"));
    }

    #[tokio::test]
    async fn authenticated_canvas_route_renders_the_interactive_surface() {
        let app = test_app(true).await;
        let anonymous = app
            .clone()
            .oneshot(Request::get("/canvas").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(anonymous.status(), StatusCode::SEE_OTHER);
        assert_eq!(anonymous.headers()[header::LOCATION], "/login");

        let login = app.clone().oneshot(login_request(PASSWORD)).await.unwrap();
        let canvas = app
            .oneshot(
                Request::get("/canvas")
                    .header(header::COOKIE, session_cookie(&login))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(canvas.status(), StatusCode::OK);
        let body = to_bytes(canvas.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("__nx_root__"));
        assert!(body.contains("/dist/"));
    }

    #[tokio::test]
    async fn integration_sync_page_seeds_status_from_extension_context() {
        let app = test_app(true).await;
        let login = app.clone().oneshot(login_request(PASSWORD)).await.unwrap();
        let page = app
            .oneshot(
                Request::get("/integrations/tmo/sync")
                    .header(header::COOKIE, session_cookie(&login))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(page.status(), StatusCode::OK);

        let body = to_bytes(page.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        let marker = r#"<script type="application/json" id="__nx_seeds__">"#;
        let seed_start = body.find(marker).expect("status seed script") + marker.len();
        let seed_end = body[seed_start..]
            .find("</script>")
            .map(|offset| seed_start + offset)
            .expect("status seed script end");
        let seeds: serde_json::Value = serde_json::from_str(&body[seed_start..seed_end]).unwrap();

        assert_eq!(
            seeds[0]["key"],
            serde_json::json!(["/integrations/tmo/sync/status"])
        );
        assert_eq!(seeds[0]["data"], serde_json::json!({ "run": null }));
    }

    #[test]
    fn generated_route_registry_contains_canvas_react_page() {
        let registry = generated_registry();
        let canvas = registry
            .entries
            .iter()
            .find(|entry| entry.path == "/canvas")
            .expect("Canvas route is generated from app/canvas/page.tsx");
        assert!(canvas.page.is_some());
        assert!(canvas.methods.is_empty());
    }

    #[test]
    fn generated_route_registry_contains_only_the_cron_get_handler() {
        let registry = generated_registry();
        let cron = registry
            .entries
            .iter()
            .find(|entry| entry.path == "/internal/cron")
            .expect("cron route is generated from app/internal/cron/route.rs");
        assert_eq!(cron.methods.len(), 1);
        assert_eq!(cron.methods[0].0, axum::http::Method::GET);
    }

    #[test]
    fn generated_route_registry_contains_activity_refresh_post() {
        let registry = generated_registry();
        let refresh = registry
            .entries
            .iter()
            .find(|entry| entry.path == "/api/integrations/refresh-if-stale")
            .expect("activity refresh route is generated from its route.rs");
        assert_eq!(refresh.methods.len(), 1);
        assert_eq!(refresh.methods[0].0, axum::http::Method::POST);
    }

    #[test]
    fn generated_openapi_contains_typed_sync_status_and_run_contracts() {
        let document = generated_openapi();
        let status = document
            .paths
            .paths
            .get("/integrations/{slug}/sync/status")
            .expect("sync status is included in the typed client contract");
        assert!(status.get.is_some());

        let run = document
            .paths
            .paths
            .get("/integrations/{slug}/sync/run")
            .expect("sync run is included in the typed client contract");
        assert!(run.post.is_some());
    }

    #[test]
    fn generated_registry_contains_direct_upload_and_media_redirect_contracts() {
        let registry = generated_registry();
        for (path, method) in [
            (
                "/api/integrations/{slug}/loans/{loan_account}/workspace/photos/upload-intent",
                axum::http::Method::POST,
            ),
            (
                "/api/integrations/{slug}/loans/{loan_account}/workspace/photos/finalize",
                axum::http::Method::POST,
            ),
            (
                "/integrations/{slug}/loans/{loan_account}/workspace",
                axum::http::Method::POST,
            ),
            ("/media/loan-workspace/{*key}", axum::http::Method::GET),
            ("/media/emails/{*key}", axum::http::Method::GET),
        ] {
            let route = registry
                .entries
                .iter()
                .find(|entry| entry.path == path)
                .unwrap_or_else(|| panic!("missing generated route {path}"));
            assert!(
                route
                    .methods
                    .iter()
                    .any(|(candidate, _)| candidate == method)
            );
        }
    }

    #[tokio::test]
    async fn media_routes_authenticate_before_the_durable_write_gate() {
        let app = test_app(true).await;
        let intent_path = "/api/integrations/tmo/loans/LN-1/workspace/photos/upload-intent";
        let anonymous = app
            .clone()
            .oneshot(
                Request::post(intent_path)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"file_name":"front.jpg","content_type":"image/jpeg","size_bytes":12}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

        let anonymous_read = app
            .clone()
            .oneshot(
                Request::get("/media/loan-workspace/LN-1/front.jpg")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(anonymous_read.status(), StatusCode::SEE_OTHER);
        assert_eq!(anonymous_read.headers()[header::LOCATION], "/login");

        let login = app.clone().oneshot(login_request(PASSWORD)).await.unwrap();
        let blocked = app
            .oneshot(
                Request::post(intent_path)
                    .header(header::COOKIE, session_cookie(&login))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"file_name":"front.jpg","content_type":"image/jpeg","size_bytes":12}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(blocked.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(blocked.into_body(), usize::MAX).await.unwrap();
        let problem: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(problem["error"], "read_only");
    }

    #[tokio::test]
    async fn fresh_database_blocks_business_mutations_after_auth() {
        let (app, context) = test_app_with_context(true).await;
        let control = OperationRepository::new(&context.connection().await.unwrap())
            .control()
            .await
            .unwrap();
        assert_eq!(control.mode, OperationMode::ReadOnly);

        let anonymous = app
            .clone()
            .oneshot(
                Request::post("/api/accounts")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"name":"Checking","is_primary":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

        let login = app.clone().oneshot(login_request(PASSWORD)).await.unwrap();
        let cookie = session_cookie(&login);
        let cross_origin = app
            .clone()
            .oneshot(
                Request::post("/api/accounts")
                    .header(header::COOKIE, &cookie)
                    .header(header::HOST, "trust-deeds.example")
                    .header(header::ORIGIN, "https://attacker.example")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"name":"Checking","is_primary":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cross_origin.status(), StatusCode::FORBIDDEN);

        let blocked = app
            .oneshot(
                Request::post("/api/accounts")
                    .header(header::COOKIE, cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"name":"Checking","is_primary":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(blocked.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(blocked.headers()[header::RETRY_AFTER], "60");
        assert_eq!(blocked.headers()[header::CONTENT_TYPE], "application/json");
        let body = to_bytes(blocked.into_body(), usize::MAX).await.unwrap();
        let problem: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(problem["error"], "read_only");
        assert_eq!(problem["message"], "Writes are temporarily disabled.");

        let connection = context.connection().await.unwrap();
        let mut rows = connection
            .query("SELECT COUNT(*) FROM account", ())
            .await
            .unwrap();
        assert_eq!(
            rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn integration_actions_are_authenticated_same_origin_and_write_gated() {
        let (app, context) = test_app_with_context(true).await;

        let anonymous_balance = app
            .clone()
            .oneshot(
                Request::post("/api/sync/balance")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"as_of_date":"2026-07-14"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(anonymous_balance.status(), StatusCode::UNAUTHORIZED);

        let login = app.clone().oneshot(login_request(PASSWORD)).await.unwrap();
        let cookie = session_cookie(&login);
        let cross_origin = app
            .clone()
            .oneshot(
                Request::post("/integrations/tmo/sync/cadence")
                    .header(header::COOKIE, &cookie)
                    .header(header::HOST, "trust-deeds.example")
                    .header(header::ORIGIN, "https://attacker.example")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("sync_cadence=every_6h"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cross_origin.status(), StatusCode::FORBIDDEN);

        let read_only = app
            .clone()
            .oneshot(
                Request::post("/integrations/tmo/sync/cadence")
                    .header(header::COOKIE, &cookie)
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("sync_cadence=every_6h"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(read_only.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(read_only.headers()[header::RETRY_AFTER], "60");

        let connection = context.connection().await.unwrap();
        connection
            .execute(
                "INSERT INTO intg_integration_connection (slug, name, provider, sync_cadence) \
                 VALUES ('tmo', 'The Mortgage Office', 'mortgage_office', 'manual')",
                (),
            )
            .await
            .unwrap();
        OperationRepository::new(&connection)
            .enable_writes("2026-07-14T18:00:00.000Z")
            .await
            .unwrap();

        let saved = app
            .clone()
            .oneshot(
                Request::post("/integrations/tmo/sync/cadence")
                    .header(header::COOKIE, &cookie)
                    .header(header::ACCEPT, "application/json")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("sync_cadence=every_12h"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(saved.status(), StatusCode::OK);
        let body = to_bytes(saved.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["sync_cadence"], "every_12h");
        let stored = crate::integrations::IntegrationRepository::new(&connection)
            .connection_by_slug("tmo")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.sync_cadence, "every_12h");
        assert!(stored.next_scheduled_at.is_some());

        // The JSON route is registered and validates the browser-local date
        // before it attempts to load credentials or contact Monarch.
        let invalid_date = app
            .oneshot(
                Request::post("/api/sync/balance")
                    .header(header::COOKIE, cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"as_of_date":"2026-02-31"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid_date.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn activity_refresh_is_authenticated_write_gated_and_cheap_when_nothing_is_due() {
        let (app, context) = test_app_with_context(true).await;
        let request_body = r#"{"as_of_date":"2026-07-14"}"#;

        let anonymous = app
            .clone()
            .oneshot(
                Request::post("/api/integrations/refresh-if-stale")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(request_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

        let login = app.clone().oneshot(login_request(PASSWORD)).await.unwrap();
        let cookie = session_cookie(&login);
        let read_only = app
            .clone()
            .oneshot(
                Request::post("/api/integrations/refresh-if-stale")
                    .header(header::COOKIE, &cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(request_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(read_only.status(), StatusCode::SERVICE_UNAVAILABLE);

        OperationRepository::new(&context.connection().await.unwrap())
            .enable_writes("2026-07-14T18:00:00.000Z")
            .await
            .unwrap();
        let checked = app
            .clone()
            .oneshot(
                Request::post("/api/integrations/refresh-if-stale")
                    .header(header::COOKIE, &cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(request_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(checked.status(), StatusCode::OK);
        let body = to_bytes(checked.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["refresh_after_seconds"], 3600);
        assert_eq!(payload["integrations"], serde_json::json!([]));

        let invalid_date = app
            .oneshot(
                Request::post("/api/integrations/refresh-if-stale")
                    .header(header::COOKIE, cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"as_of_date":"2026-02-31"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid_date.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn enabled_write_passes_and_non_api_blocks_are_retryable() {
        let (app, context) = test_app_with_context(true).await;
        let login = app.clone().oneshot(login_request(PASSWORD)).await.unwrap();
        let cookie = session_cookie(&login);

        let form = app
            .clone()
            .oneshot(
                Request::post("/future-form")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(form.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(form.headers()[header::RETRY_AFTER], "60");
        let body = to_bytes(form.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body, "Writes are temporarily disabled.");

        OperationRepository::new(&context.connection().await.unwrap())
            .enable_writes("2026-07-14T18:00:00.000Z")
            .await
            .unwrap();
        let created = app
            .oneshot(
                Request::post("/api/accounts")
                    .header(header::COOKIE, cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"name":"Checking","balance":1250,"is_primary":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn cron_secret_precedes_the_durable_scheduler_gate_and_session_auth_does_not_bypass_it() {
        let (app, context) = test_app_with_context(true).await;
        let login = app.clone().oneshot(login_request(PASSWORD)).await.unwrap();
        let cookie = session_cookie(&login);
        let connection = context.connection().await.unwrap();
        let repository = OperationRepository::new(&connection);
        repository
            .enable_writes("2026-07-14T18:00:00.000Z")
            .await
            .unwrap();

        let missing_secret = app
            .clone()
            .oneshot(
                Request::get("/internal/cron")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_secret.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            missing_secret.headers()[header::WWW_AUTHENTICATE],
            "Bearer realm=\"cron\""
        );
        let missing_body = to_bytes(missing_secret.into_body(), usize::MAX)
            .await
            .unwrap();

        let wrong_secret = app
            .clone()
            .oneshot(cron_request(Some("wrong-secret")))
            .await
            .unwrap();
        assert_eq!(wrong_secret.status(), StatusCode::UNAUTHORIZED);
        let wrong_body = to_bytes(wrong_secret.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(missing_body, wrong_body);

        let bearer_does_not_open_browser_pages = app
            .clone()
            .oneshot(
                Request::get("/")
                    .header(header::AUTHORIZATION, "Bearer test-cron-secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            bearer_does_not_open_browser_pages.status(),
            StatusCode::SEE_OTHER
        );
        assert_eq!(
            bearer_does_not_open_browser_pages.headers()[header::LOCATION],
            "/login"
        );

        let scheduler_off = app
            .clone()
            .oneshot(cron_request(Some("test-cron-secret")))
            .await
            .unwrap();
        assert_eq!(scheduler_off.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(scheduler_off.headers()[header::RETRY_AFTER], "60");
        let body = to_bytes(scheduler_off.into_body(), usize::MAX)
            .await
            .unwrap();
        let problem: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(problem["error"], "scheduler_disabled");

        repository
            .set_scheduler_enabled(true, "2026-07-14T18:01:00.000Z")
            .await
            .unwrap();
        let not_configured = app
            .clone()
            .oneshot(cron_request(Some("test-cron-secret")))
            .await
            .unwrap();
        assert_eq!(not_configured.status(), StatusCode::OK);
        let body = to_bytes(not_configured.into_body(), usize::MAX)
            .await
            .unwrap();
        let problem: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            problem["integrations"][0]["outcome"],
            "not_configured",
            "an unconfigured integration is a healthy per-entry no-op"
        );

        connection
            .execute(
                "INSERT INTO intg_integration_connection (slug, name, provider, sync_cadence) \
                 VALUES ('tmo', 'The Mortgage Office', 'mortgage_office', 'manual')",
                (),
            )
            .await
            .unwrap();
        let manual = app
            .clone()
            .oneshot(cron_request(Some("test-cron-secret")))
            .await
            .unwrap();
        assert_eq!(manual.status(), StatusCode::OK);
        let body = to_bytes(manual.into_body(), usize::MAX).await.unwrap();
        let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(result["integrations"][0]["provider"], "tmo");
        assert_eq!(result["integrations"][0]["outcome"], "manual");

        connection
            .execute("DELETE FROM operation_control WHERE id = 1", ())
            .await
            .unwrap();
        // Wrong credentials still return the same 401 without consulting the
        // now-broken operation-control state.
        let unauthorized = app
            .clone()
            .oneshot(cron_request(Some("still-wrong")))
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let head_is_never_mutating = app
            .clone()
            .oneshot(
                Request::head("/internal/cron")
                    .header(header::AUTHORIZATION, "Bearer test-cron-secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            head_is_never_mutating.status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(head_is_never_mutating.headers()[header::ALLOW], "GET");

        let unavailable_cron = app
            .clone()
            .oneshot(cron_request(Some("test-cron-secret")))
            .await
            .unwrap();
        assert_eq!(unavailable_cron.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(unavailable_cron.headers()[header::RETRY_AFTER], "60");
        let body = to_bytes(unavailable_cron.into_body(), usize::MAX)
            .await
            .unwrap();
        let problem: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(problem["error"], "service_unavailable");

        let unavailable = app
            .oneshot(
                Request::post("/api/accounts")
                    .header(header::COOKIE, cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"name":"Never written"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unavailable.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(unavailable.headers()[header::RETRY_AFTER], "60");
        let body = to_bytes(unavailable.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains(r#""error":"service_unavailable""#));
        assert!(!body.contains("singleton"));
        assert!(!body.contains("operation-control"));
    }

    #[tokio::test]
    async fn local_bootstrap_is_the_explicit_write_enable_path() {
        let context = test_context(false).await;
        bootstrap_local_context(&context, Some(EMAIL), Some(PASSWORD))
            .await
            .unwrap();
        let control = OperationRepository::new(&context.connection().await.unwrap())
            .control()
            .await
            .unwrap();
        assert_eq!(control.mode, OperationMode::Enabled);
        assert!(!control.scheduler_enabled);
    }

    #[test]
    fn generated_mutation_inventory_is_default_deny() {
        let registry = generated_registry();
        let mut protected = 0;
        for entry in registry.entries {
            for (method, _) in entry.methods {
                if matches!(
                    method,
                    axum::http::Method::POST
                        | axum::http::Method::PUT
                        | axum::http::Method::PATCH
                        | axum::http::Method::DELETE
                ) {
                    let expected = if entry.path == "/login" || entry.path == "/logout" {
                        GateRequirement::Exempt
                    } else {
                        protected += 1;
                        GateRequirement::Writes
                    };
                    assert_eq!(
                        requirement(&method, &entry.path),
                        expected,
                        "{}",
                        entry.path
                    );
                }
            }
        }
        assert!(
            protected >= 10,
            "expected the finance mutations in inventory"
        );

        for path in [
            "/api/future",
            "/integrations/tmo",
            "/sync/tmo",
            "/webhooks/resend",
        ] {
            assert_eq!(
                requirement(&axum::http::Method::POST, path),
                GateRequirement::Writes,
                "{path}",
            );
        }
        assert_eq!(
            requirement(&axum::http::Method::GET, "/internal/cron"),
            GateRequirement::Scheduler,
        );
    }

    #[tokio::test]
    async fn logout_revokes_old_cookie() {
        let app = test_app(true).await;
        let login = app.clone().oneshot(login_request(PASSWORD)).await.unwrap();
        let cookie = session_cookie(&login);

        let logout = app
            .clone()
            .oneshot(
                Request::post("/logout")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(logout.status(), StatusCode::SEE_OTHER);
        assert_eq!(logout.headers()[header::LOCATION], "/login");

        let replay = app
            .oneshot(
                Request::get("/")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(replay.status(), StatusCode::SEE_OTHER);
        assert_eq!(replay.headers()[header::LOCATION], "/login");
    }

    #[tokio::test]
    async fn a_stale_save_cannot_recreate_a_deleted_session() {
        let context = test_context(false).await;
        let store = LibsqlSessionStore::new(context.clone());
        let mut record = Record {
            id: Id::default(),
            data: Default::default(),
            expiry_date: OffsetDateTime::now_utc() + Duration::hours(1),
        };

        store.create(&mut record).await.unwrap();
        store.delete(&record.id).await.unwrap();

        assert!(store.save(&record).await.is_err());
        assert!(store.load(&record.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn health_is_public() {
        let app = test_app(false).await;
        for path in ["/health", "/healthz"] {
            let response = app
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_security_headers(&response);
        }
    }

    #[tokio::test]
    async fn readiness_is_public_and_checks_login_dependency() {
        let not_ready = test_app(false)
            .await
            .oneshot(Request::get("/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(not_ready.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_security_headers(&not_ready);
        let body = to_bytes(not_ready.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body, "not ready");

        let ready = test_app(true)
            .await
            .oneshot(Request::get("/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(ready.status(), StatusCode::OK);
        assert_security_headers(&ready);
        let body = to_bytes(ready.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body, "ready");
    }

    #[tokio::test]
    async fn request_ids_are_validated_and_emitted_on_fallbacks() {
        let app = test_app(false).await;
        let accepted = app
            .clone()
            .oneshot(
                Request::get("/health")
                    .header("x-request-id", "client-123_ABC.xyz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(accepted.headers()["x-request-id"], "client-123_ABC.xyz");

        let replaced = app
            .clone()
            .oneshot(
                Request::get("/health")
                    .header("x-request-id", "not valid")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let generated = replaced.headers()["x-request-id"].to_str().unwrap();
        assert!(generated.starts_with("td_"));
        assert_ne!(generated, "not valid");

        let fallback = app
            .oneshot(Request::get("/does-not-exist").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(fallback.status(), StatusCode::SEE_OTHER);
        assert_security_headers(&fallback);
    }

    #[tokio::test]
    async fn unknown_routes_keep_auth_and_content_negotiation_semantics() {
        let app = test_app(true).await;

        let anonymous_page = app
            .clone()
            .oneshot(Request::get("/missing").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(anonymous_page.status(), StatusCode::SEE_OTHER);
        assert_eq!(anonymous_page.headers()[header::LOCATION], "/login");

        let anonymous_api = app
            .clone()
            .oneshot(Request::get("/api/missing").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(anonymous_api.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(anonymous_api.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body, r#"{"error":"unauthorized"}"#);

        let login = app.clone().oneshot(login_request(PASSWORD)).await.unwrap();
        let cookie = session_cookie(&login);

        let page = app
            .clone()
            .oneshot(
                Request::get("/missing")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(page.status(), StatusCode::NOT_FOUND);
        assert_security_headers(&page);
        let body = to_bytes(page.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("That page isn't here"));

        let api = app
            .oneshot(
                Request::get("/api/missing")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(api.status(), StatusCode::NOT_FOUND);
        assert_eq!(api.headers()[header::CONTENT_TYPE], "application/json");
        assert_security_headers(&api);
        let body = to_bytes(api.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body, r#"{"error":"not_found"}"#);
    }

    #[tokio::test]
    async fn local_static_files_stay_public_and_skip_private_no_store() {
        let app = test_app(false).await;
        let asset = app
            .clone()
            .oneshot(
                Request::get("/static/style.css")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(asset.status(), StatusCode::OK);
        assert!(asset.headers().get(header::LOCATION).is_none());
        assert_ne!(
            asset
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("private, no-store")
        );
        assert_security_headers(&asset);

        let missing = app
            .oneshot(
                Request::get("/static/does-not-exist.css")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert!(missing.headers().get(header::LOCATION).is_none());
        assert_security_headers(&missing);
    }

    #[tokio::test]
    async fn login_form_body_is_bounded() {
        let oversized = format!("email={}&password=x", "a".repeat(70 * 1_024));
        let response = test_app(false)
            .await
            .oneshot(
                Request::post("/login")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(oversized))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_security_headers(&response);
    }

    #[tokio::test]
    async fn anonymous_api_is_json_unauthorized() {
        let response = test_app(false)
            .await
            .oneshot(Request::get("/api/forecast").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body, r#"{"error":"unauthorized"}"#);
    }

    #[tokio::test]
    async fn htmx_anonymous_request_gets_full_page_redirect_instruction() {
        let response = test_app(false)
            .await
            .oneshot(
                Request::get("/")
                    .header("HX-Request", "true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["HX-Redirect"], "/login");
    }

    #[tokio::test]
    async fn cross_origin_login_is_rejected() {
        let response = test_app(true)
            .await
            .oneshot(
                Request::post("/login")
                    .header(header::HOST, "trust-deeds.example")
                    .header(header::ORIGIN, "https://attacker.example")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "email=admin%40example.com&password={}",
                        PASSWORD.replace(' ', "+")
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(response.headers().get(header::SET_COOKIE).is_none());
    }

    #[tokio::test]
    async fn same_origin_fetch_metadata_survives_a_serverless_host_rewrite() {
        let response = test_app(true)
            .await
            .oneshot(
                Request::post("/login")
                    .header(header::HOST, "internal-function.example")
                    .header(header::ORIGIN, "https://trust-deeds.example")
                    .header("sec-fetch-site", "same-origin")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "email=admin%40example.com&password={}",
                        PASSWORD.replace(' ', "+")
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert!(response.headers().get(header::SET_COOKIE).is_some());
    }

    #[derive(Clone, Debug)]
    struct FaultStore;

    fn store_error() -> session_store::Error {
        session_store::Error::Backend("injected store failure".into())
    }

    #[async_trait]
    impl SessionStore for FaultStore {
        async fn create(&self, _record: &mut Record) -> session_store::Result<()> {
            Err(store_error())
        }

        async fn save(&self, _record: &Record) -> session_store::Result<()> {
            Err(store_error())
        }

        async fn load(&self, _session_id: &Id) -> session_store::Result<Option<Record>> {
            Err(store_error())
        }

        async fn delete(&self, _session_id: &Id) -> session_store::Result<()> {
            Err(store_error())
        }
    }

    #[tokio::test]
    async fn session_store_fault_returns_service_unavailable() {
        let app = router_with_store(
            test_context(false).await,
            FaultStore,
            false,
            Arc::new(CredentialCipher::new("test-router-key").unwrap()),
            CronAuthenticator::new(Some("test-cron-secret")),
            Arc::new(MediaService::disabled()),
            Arc::new(ResendService::disabled()),
        );
        let invalid_session = format!("__td_session={}", Id::default());
        let rejected_cron = app
            .clone()
            .oneshot(
                Request::get("/internal/cron")
                    .header(header::COOKIE, &invalid_session)
                    .header(header::AUTHORIZATION, "Bearer wrong-secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rejected_cron.status(), StatusCode::UNAUTHORIZED);

        let response = app
            .oneshot(
                Request::get("/")
                    .header(header::COOKIE, invalid_session)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
