#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3002);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;

    let app = trust_deeds_nextrs::configured_router().await?;
    println!("Trust Deeds NextRS listening on http://localhost:{port}");
    axum::serve(listener, app).await?;

    Ok(())
}
