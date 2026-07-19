#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    trust_deeds::init_tracing();
    dotenvy::dotenv().ok();
    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3003);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;

    let app = trust_deeds::configured_router().await?;
    #[cfg(debug_assertions)]
    let app = app.layer(tower_livereload::LiveReloadLayer::new());

    println!("Trust Deeds NextRS listening on http://localhost:{port}");
    axum::serve(listener, app).await?;

    Ok(())
}
