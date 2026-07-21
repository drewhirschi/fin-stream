use axum::{
    extract::Request,
    http::HeaderName,
    middleware::{Next, from_fn},
    response::Response,
};
use nextrs::vercel::StreamingVercelLayer;
use tower::ServiceBuilder;

const DEBUG_SERVER_TIMING: HeaderName = HeaderName::from_static("x-debug-server-timing");
const SERVER_TIMING: HeaderName = HeaderName::from_static("server-timing");

async fn mirror_server_timing(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    if let Some(value) = response.headers().get(SERVER_TIMING).cloned() {
        response.headers_mut().insert(DEBUG_SERVER_TIMING, value);
    }
    response
}

#[tokio::main]
async fn main() -> Result<(), vercel_runtime::Error> {
    trust_deeds::init_tracing();
    let router = trust_deeds::configured_router()
        .await
        .map_err(|error| vercel_runtime::Error::from(error.to_string()))?
        // Temporary diagnostic: Vercel currently drops the standards-based
        // header while forwarding other application response headers.
        .layer(from_fn(mirror_server_timing));
    let app = ServiceBuilder::new()
        .layer(StreamingVercelLayer::new())
        .service(router);

    vercel_runtime::run(app).await
}
