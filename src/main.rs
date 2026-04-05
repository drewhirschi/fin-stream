use axum::Router;
use tower_http::services::ServeDir;
use std::sync::Arc;
use tokio::sync::Mutex;

use trust_deeds::{AppState, config, db, routes};

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

    let state = Arc::new(AppState {
        db: pool,
        sync_status: Mutex::new(None),
    });

    let app = Router::new()
        .merge(routes::health::router())
        .merge(routes::pages::router())
        .merge(routes::sync::router())
        .merge(routes::api::router())
        .nest_service("/static", ServeDir::new("static"))
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
