//! Erweiterung des Fiori-Servers fuer die Video-Thumbnails.
//!
//! * `GET /ext/video/thumbnail/{video_id}/{n}`: same-origin Proxy. Ermittelt
//!   die Agent-ID des Videos aus dem Store und holt das Bild vom Thumbnail-Dienst
//!   (`{THUMBNAIL_BASE_URL}/{agent}/thumbnail/{video}/{n}`). Vermeidet
//!   CORS/Private-Network-Probleme im Browser.
//! * `GET /ext/video/fragment/Thumbnails.fragment.xml`: Custom Section der
//!   Video-Object-Page; ersetzt die Standard-Facet der Navigation `_VideoThumbnails`.
//!
//! Der fiori-server kennt diese Datei nicht; sie haengt sich ueber
//! [`fiori_server::extension::Extension`] ein.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use data_store::EntityKey;
use fiori_server::{extension::Extension, AppState};
use odata::query::ODataQuery;
use serde_json::{json, Value};
use uuid::Uuid;

/// Standard-Upstream, ueberschreibbar ueber `THUMBNAIL_BASE_URL`.
pub const DEFAULT_UPSTREAM: &str = "http://nuc:9000/proxy";
const FRAGMENT: &str = include_str!("../fragments/Thumbnails.fragment.xml");
const PREVIEW_FRAGMENT: &str = include_str!("../fragments/PreviewColumn.fragment.xml");
const VIDEO_SET: &str = "VideoSet";

pub struct VideoThumbnails {
    upstream: String,
}

impl VideoThumbnails {
    pub fn from_env() -> Self {
        Self {
            upstream: std::env::var("THUMBNAIL_BASE_URL")
                .unwrap_or_else(|_| DEFAULT_UPSTREAM.to_string()),
        }
    }
}

struct ProxyState {
    app: AppState,
    client: reqwest::Client,
    upstream: String,
}

impl Extension for VideoThumbnails {
    fn routes(&self, app: AppState) -> Router {
        let state = Arc::new(ProxyState {
            app,
            client: reqwest::Client::new(),
            upstream: self.upstream.clone(),
        });
        Router::new()
            .route("/ext/video/thumbnail/{video_id}/{n}", get(thumbnail))
            .route("/ext/video/fragment/Thumbnails.fragment.xml", get(fragment))
            .route(
                "/ext/video/fragment/PreviewColumn.fragment.xml",
                get(preview_fragment),
            )
            .route("/apps/Video/i18n/i18n.properties", get(i18n_default))
            .route("/apps/Video/i18n/i18n_de.properties", get(i18n_de))
            .route("/apps/Video/i18n/i18n_en.properties", get(i18n_en))
            .with_state(state)
    }

    fn ui5_paths(&self) -> Vec<(String, String)> {
        vec![("video.ext".into(), "/ext/video".into())]
    }

    fn list_columns(&self, entity: &str) -> Option<Value> {
        if entity != "Video" {
            return None;
        }
        Some(json!({
            "VideoPreview": {
                "header": "{{previewColumnTitle}}",
                "width": "120px",
                "horizontalAlign": "Begin",
                "position": { "placement": "Before", "anchor": "name" },
                "template": "video.ext.fragment.PreviewColumn"
            }
        }))
    }

    fn object_page_content(&self, entity: &str) -> Option<Value> {
        if entity != "Video" {
            return None;
        }
        Some(json!({ "body": { "sections": {
            // Standard-Facet der Navigation _VideoThumbnails (ID = <nav>Section) ausblenden.
            "_VideoThumbnailsSection": { "visible": false },
            "VideoThumbnails": {
                "template": "video.ext.fragment.Thumbnails",
                "title": "{{thumbnailsTitle}}",
                "position": { "placement": "After", "anchor": "GeneralSection" }
            }
        }}}))
    }
}

const I18N_DEFAULT: &str = include_str!("../i18n/i18n.properties");
const I18N_DE: &str = include_str!("../i18n/i18n_de.properties");
const I18N_EN: &str = include_str!("../i18n/i18n_en.properties");

fn properties(body: &'static str) -> Response {
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
}

async fn i18n_default() -> Response {
    properties(I18N_DEFAULT)
}

async fn i18n_de() -> Response {
    properties(I18N_DE)
}

async fn i18n_en() -> Response {
    properties(I18N_EN)
}

async fn fragment() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        FRAGMENT,
    )
}

async fn preview_fragment() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        PREVIEW_FRAGMENT,
    )
}

/// Fehlerantwort mit Begruendung im Body und Log-Eintrag mit allen Details.
fn fail(status: StatusCode, video: &str, n: u32, reason: impl std::fmt::Display) -> Response {
    tracing::warn!(video, n, %status, "thumbnail: {reason}");
    (status, format!("thumbnail {video}/{n}: {reason}\n")).into_response()
}

async fn thumbnail(
    State(st): State<Arc<ProxyState>>,
    user: Option<axum::Extension<fiori_server::auth::CurrentUser>>,
    Path((video_id, n)): Path<(String, u32)>,
) -> Response {
    let user = user.map(|u| u.0 .0).unwrap_or_else(|| "-".into());
    let id = match Uuid::parse_str(&video_id) {
        Ok(id) => id,
        Err(e) => return fail(StatusCode::BAD_REQUEST, &video_id, n, format!("keine gueltige UUID ({e})")),
    };
    tracing::debug!(%user, video = %id, n, "thumbnail requested");

    let agent = match agent_of(&st.app, id).await {
        Ok(agent) => agent,
        Err(reason) => return fail(StatusCode::NOT_FOUND, &video_id, n, reason),
    };

    let url = format!("{}/{}/thumbnail/{}/{}", st.upstream, agent, id, n);
    let resp = match st.client.get(&url).send().await {
        Ok(resp) => resp,
        Err(e) => {
            return fail(
                StatusCode::BAD_GATEWAY,
                &video_id,
                n,
                format!("Upstream nicht erreichbar: GET {url}: {e:?}"),
            )
        }
    };
    let upstream_status = resp.status();
    if !upstream_status.is_success() {
        // 404 vom Upstream bleibt 404 (Bild existiert nicht); alles andere ist ein Gateway-Fehler.
        let status = if upstream_status == reqwest::StatusCode::NOT_FOUND {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_GATEWAY
        };
        return fail(
            status,
            &video_id,
            n,
            format!("Upstream antwortet {upstream_status} fuer GET {url}"),
        );
    }
    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned()
        .unwrap_or_else(|| header::HeaderValue::from_static("image/jpeg"));
    let bytes = match resp.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => {
            return fail(
                StatusCode::BAD_GATEWAY,
                &video_id,
                n,
                format!("Upstream-Body unlesbar von {url}: {e:?}"),
            )
        }
    };
    (
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CACHE_CONTROL,
                header::HeaderValue::from_static("private, max-age=86400"),
            ),
        ],
        bytes,
    )
        .into_response()
}

/// Agent-ID des Videos. Liest zuerst die aktive Version, sonst den Draft.
/// Im Fehlerfall die Begruendung als Text.
async fn agent_of(app: &AppState, id: Uuid) -> Result<String, String> {
    let mut last_err = String::new();
    for active in [true, false] {
        let key = EntityKey::new(id, active);
        match app
            .store
            .read_entity(VIDEO_SET, &key, &ODataQuery::default())
            .await
        {
            Ok(record) => {
                return match record.get("agent_ID") {
                    Some(v) => v
                        .as_str()
                        .map(str::to_string)
                        .ok_or_else(|| format!("agent_ID ist kein Text: {v}")),
                    None => Err(format!(
                        "Video {id} hat kein Feld agent_ID (active={active})"
                    )),
                };
            }
            Err(e) => last_err = format!("Video {id} nicht lesbar (active={active}): {e:?}"),
        }
    }
    Err(last_err)
}
