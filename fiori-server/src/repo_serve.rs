//! Auslieferung statischer Dateien aus einem [`FileRepository`].
//!
//! Ersetzt `tower_http::ServeDir`, das nur das Dateisystem kennt. Die Inhalte
//! stammen aus dem Repository (Verzeichnis oder ZIP im Speicher).

use std::sync::Arc;

use axum::{
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use file_repo::FileRepository;

/// Datei `raw` (URL-Pfad ohne fuehrenden `/`) aus dem Repository liefern.
/// Verzeichnisse liefern `index.html`. Unsichere Pfade ergeben 404.
pub fn serve(repo: &dyn FileRepository, raw: &str) -> Response {
    let mut path = raw.trim_start_matches('/').to_string();
    if path.is_empty() || path.ends_with('/') {
        path.push_str("index.html");
    }
    match repo.read(&path) {
        Ok(bytes) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            (
                [(header::CONTENT_TYPE, mime.essence_str().to_string())],
                bytes,
            )
                .into_response()
        }
        Err(file_repo::RepoError::NotFound(_)) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Fallback-Handler fuer die Web-Wurzel (`FIORI_ROOT`).
pub fn fallback(
    repo: Arc<dyn FileRepository>,
) -> impl Fn(Uri) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> + Clone {
    move |uri: Uri| {
        let repo = repo.clone();
        Box::pin(async move {
            let decoded = percent_encoding::percent_decode_str(uri.path()).decode_utf8_lossy();
            serve(repo.as_ref(), &decoded)
        })
    }
}
