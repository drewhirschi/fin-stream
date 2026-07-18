use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::IntoResponse,
};

pub async fn get(_request: Request<Body>) -> impl IntoResponse {
    (StatusCode::OK, "ok")
}
