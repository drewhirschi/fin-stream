use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::IntoResponse,
};

/// Process liveness only. Dependency and deployment readiness live at
/// `/ready`, so a transient database fault cannot make this probe lie about
/// whether the function runtime is serving HTTP.
pub async fn get(_request: Request<Body>) -> impl IntoResponse {
    (StatusCode::OK, "ok")
}
