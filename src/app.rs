//! Shared application wiring: configuration, service construction, and the
//! Axum `Router` both entry points consume (`src/main.rs` locally,
//! `api/index.rs` on Vercel). Route files under `app/` stay thin adapters;
//! this module is the nextrs `src/app.rs` convention.

use axum::{
    Router,
    extract::{DefaultBodyLimit, Extension},
    middleware::{from_fn, from_fn_with_state},
};
use crate::{config, cron_auth, crypto, db, media, middleware, resend, write_gate};
#[cfg(feature = "local-db")]
use crate::{finance, operations};
use crate::session_store::LibsqlSessionStore;
use std::sync::Arc;
use time::Duration;
#[cfg(feature = "local-server")]
use tower_http::services::{ServeDir, ServeFile};
use tower_sessions::{Expiry, SessionManagerLayer, SessionStore, cookie::SameSite};

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
pub(crate) async fn bootstrap_local_context(
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

pub(crate) fn router_with_store<S>(
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
        crate::generated_registry(),
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

