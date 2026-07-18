use nextrs::vercel::StreamingVercelLayer;
use tower::ServiceBuilder;

#[tokio::main]
async fn main() -> Result<(), vercel_runtime::Error> {
    let router = trust_deeds::configured_router()
        .await
        .map_err(|error| vercel_runtime::Error::from(error.to_string()))?;
    let app = ServiceBuilder::new()
        .layer(StreamingVercelLayer::new())
        .service(router);

    vercel_runtime::run(app).await
}
