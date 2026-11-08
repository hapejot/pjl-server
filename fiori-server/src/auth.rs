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
    // Entwurfsverwaltung (`InProcessByUser`) liest denselben Benutzer ueber den Store.
    let principal = user.clone();
    Ok(data_store::datastore::CURRENT_PRINCIPAL
        .scope(
            principal,
            crate::system_user::CURRENT_USER.scope(user, next.run(req)),
        )
        .await)
}

#[cfg(test)]
mod tests;
