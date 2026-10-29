//! Benutzeridentitaet aus dem Header eines vorgeschalteten Auth-Proxys.
//!
//! Der Server prueft keine Anmeldung selbst. Ein Reverse Proxy (z. B. Caddy mit
//! oauth2-proxy) authentifiziert den Benutzer und setzt den Header
//! `AUTH_USER_HEADER` (Standard `X-Forwarded-User`). Dieser Header wird hier
//! gelesen und als [`CurrentUser`] in die Request-Extensions gelegt.
//!
//! Ohne Header gilt der Default User (`none@localhost`), siehe [`crate::system_user`].
//!
//! Wichtig: Der Proxy muss den Header von eingehenden Anfragen entfernen und
//! selbst setzen. Sonst kann ein Client die Identitaet frei vorgeben.

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};

use crate::config::ServerConfig;

/// Angemeldeter Benutzer dieser Anfrage. Als Request-Extension verfuegbar:
/// `Option<axum::Extension<CurrentUser>>` in Handlern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentUser(pub String);

/// Middleware: liest den Benutzer-Header, legt [`CurrentUser`] ab und
/// verlangt ihn bei `auth_required`.
pub async fn identity(
    State(config): State<std::sync::Arc<ServerConfig>>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let user = req
        .headers()
        .get(config.auth_user_header.as_str())
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);

    if user.is_none() && config.auth_required {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if let Some(name) = &user {
        req.extensions_mut().insert(CurrentUser(name.clone()));
    }
    // Der Benutzer gilt fuer die ganze Anfrage (Hooks lesen ihn ueber CURRENT_USER).
    Ok(crate::system_user::CURRENT_USER.scope(user, next.run(req)).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, routing::get, Router};
    use std::sync::Arc;
    use tower::ServiceExt;

    fn app(required: bool) -> Router {
        let mut config = ServerConfig::defaults(crate::config::ServerMode::Runtime);
        config.auth_required = required;
        Router::new()
            .route(
                "/who",
                get(|user: Option<axum::Extension<CurrentUser>>| async move {
                    user.map(|u| u.0 .0).unwrap_or_else(|| "anonymous".into())
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
}
