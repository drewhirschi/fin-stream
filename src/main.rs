use axum::{Router, middleware};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::services::ServeDir;
use tower_sessions::{Expiry, SessionManagerLayer, cookie::SameSite};
use tower_sessions_sqlx_store::PostgresStore;

use trust_deeds::{AppState, auth, config, db, routes};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,trust_deeds=debug".into()),
        )
        .init();

    let pool = db::init().await?;

    // Ensure default accounts, streams, views, and recurring schedules exist on startup.
    db::streams::ensure_default_configuration(&pool).await?;

    // Seed the initial admin user from ADMIN_EMAIL/ADMIN_PASSWORD if set.
    db::ensure_admin_user(&pool).await?;

    // Set up Postgres-backed session store. The table is created on first run.
    let session_store = PostgresStore::new(pool.clone());
    session_store.migrate().await?;

    let state = Arc::new(AppState {
        db: pool,
        sync_status: Mutex::new(None),
    });

    // Start the background cron scheduler for integration syncs.
    tokio::spawn(trust_deeds::scheduler::run(state.clone()));

    let session_layer = SessionManagerLayer::new(session_store)
        .with_name("__td_session")
        .with_same_site(SameSite::Lax)
        .with_http_only(true)
        .with_secure(!cfg!(debug_assertions))
        .with_expiry(Expiry::OnInactivity(time::Duration::days(7)));

    // Public routes — reachable without authentication.
    //   - health: liveness probes
    //   - webhooks: Resend (verified by Svix signature in the handler)
    //   - auth: login/logout pages
    let public = Router::new()
        .merge(routes::health::router())
        .merge(routes::webhooks::router())
        .merge(routes::auth::router());

    // Protected routes — require a valid session via the require_auth middleware.
    let protected = Router::new()
        .merge(routes::media::router())
        .merge(routes::pages::router())
        .merge(routes::sync::router())
        .merge(routes::api::router())
        .merge(routes::health::protected_router())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ));

    let app = Router::new()
        .merge(public)
        .merge(protected)
        .nest_service("/static", ServeDir::new("static"))
        .fallback(routes::pages::not_found)
        .layer(session_layer)
        .with_state(state.clone());

    #[cfg(debug_assertions)]
    let app = {
        let livereload = tower_livereload::LiveReloadLayer::new();
        app.layer(livereload)
    };

    let host = config::get_host();
    let port = config::get_port();
    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("listening on http://{addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
    tracing::info!("shutting down");
}
