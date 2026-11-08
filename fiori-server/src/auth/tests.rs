use super::*;
use axum::{Router, body::Body, routing::get};
use std::sync::Arc;
use tower::ServiceExt;

fn app(required: bool) -> Router {
    let mut config = ServerConfig::defaults(crate::config::ServerMode::Runtime);
    config.auth_required = required;
    Router::new()
        .route(
            "/who",
            get(|user: Option<axum::Extension<CurrentUser>>| async move {
                user.map(|u| u.0.0).unwrap_or_else(|| "anonymous".into())
            }),
        )
        .layer(axum::middleware::from_fn_with_state(
            Arc::new(config),
            identity,
        ))
}

async fn call(app: Router, header: Option<&str>) -> (StatusCode, String) {
    let mut req = axum::http::Request::builder().uri("/who");
    if let Some(v) = header {
        req = req.header("X-Forwarded-User", v);
    }
    let res = app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn header_is_passed_to_handler() {
    let (status, body) = call(app(false), Some("peter@example.org")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "peter@example.org");
}

#[tokio::test]
async fn missing_header_is_allowed_when_not_required() {
    let (status, body) = call(app(false), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "anonymous");
}

#[tokio::test]
async fn missing_header_is_rejected_when_required() {
    let (status, _) = call(app(true), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn blank_header_counts_as_missing() {
    let (status, _) = call(app(true), Some("   ")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
