//! Video-Player auf der Object Page.
//!
//! * `GET /ext/video/stream/{video_id}`: same-origin Proxy. Ermittelt Agent-ID und
//!   `file_path` des Videos aus dem Store und holt die Datei vom Upstream:
//!   `{VIDEO_STREAM_BASE_URL}/{agent}/stream/{base64(file_path)}`. `Range`-Anfragen
//!   werden durchgereicht, damit der Browser spulen kann.
//! * `GET /ext/video/fragment/Player.fragment.xml`: Custom Section der Object Page
//!   mit einem HTML5-`<video>`-Element, das auf den Proxy zeigt.
//!
//! Der fiori-server kennt diese Datei nicht; sie haengt sich ueber
//! [`fiori_server::extension::Extension`] ein.

use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use data_store::EntityKey;
use fiori_server::{extension::Extension, AppState};
use odata::query::ODataQuery;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::thumbnails::DEFAULT_UPSTREAM;

const VIDEO_SET: &str = "VideoSet";
const FRAGMENT: &str = include_str!("../fragments/Player.fragment.xml");

pub struct VideoPlayer {
    upstream: String,
}

impl VideoPlayer {
    /// Upstream aus `VIDEO_STREAM_BASE_URL`, Default wie beim Thumbnail-Dienst.
    pub fn from_env() -> Self {
        Self {
            upstream: std::env::var("VIDEO_STREAM_BASE_URL")
                .unwrap_or_else(|_| DEFAULT_UPSTREAM.to_string()),
        }
    }
}

struct PlayerState {
    app: AppState,
    client: reqwest::Client,
    upstream: String,
}

impl Extension for VideoPlayer {
    fn routes(&self, app: AppState) -> Router {
        let state = Arc::new(PlayerState {
            app,
            client: reqwest::Client::new(),
            upstream: self.upstream.clone(),
        });
        Router::new()
            .route("/ext/video/stream/{video_id}", get(stream))
            .route("/ext/video/fragment/Player.fragment.xml", get(fragment))
            .with_state(state)
    }

    fn object_page_content(&self, entity: &str) -> Option<Value> {
        if entity != "Video" {
            return None;
        }
        Some(json!({ "body": { "sections": {
            "VideoPlayer": {
                "title": "Video",
                "template": "video.ext.fragment.Player",
                "position": { "placement": "After", "anchor": "GeneralSection" }
            }
        }}}))
    }
}

async fn fragment() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/xml; charset=utf-8")], FRAGMENT)
}

async fn stream(
    State(st): State<Arc<PlayerState>>,
    Path(video_id): Path<String>,
    req_headers: HeaderMap,
) -> Result<Response, StatusCode> {
    let id = Uuid::parse_str(&video_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let (agent, file_path) = source_of(&st.app, id).await.ok_or(StatusCode::NOT_FOUND)?;

    let url = format!(
        "{}/{}/stream/{}",
        st.upstream,
        agent,
        STANDARD.encode(file_path.as_bytes())
    );
    let mut req = st.client.get(&url);
    if let Some(range) = req_headers.get(header::RANGE) {
        req = req.header(header::RANGE, range.clone());
    }
    let resp = req.send().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    let status = resp.status();
    if !(status.is_success() || status == reqwest::StatusCode::PARTIAL_CONTENT) {
        return Err(StatusCode::NOT_FOUND);
    }

    let mut out = HeaderMap::new();
    for name in [
        header::CONTENT_TYPE,
        header::CONTENT_LENGTH,
        header::CONTENT_RANGE,
        header::ACCEPT_RANGES,
    ] {
        if let Some(v) = resp.headers().get(&name) {
            out.insert(name, v.clone());
        }
    }
    out.entry(header::CONTENT_TYPE)
        .or_insert(HeaderValue::from_static("video/mp4"));
    out.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));

    let body = Body::from_stream(resp.bytes_stream());
    Ok((
        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK),
        out,
        body,
    )
        .into_response())
}

/// Agent-ID und Dateipfad des Videos. Liest zuerst die aktive Version, sonst den Draft.
async fn source_of(app: &AppState, id: Uuid) -> Option<(String, String)> {
    for active in [true, false] {
        let key = EntityKey::new(id, active);
        if let Ok(record) = app
            .store
            .read_entity(VIDEO_SET, &key, &ODataQuery::default())
            .await
        {
            let agent = record.get("agent_ID")?.as_str()?.to_string();
            let path = record.get("file_path")?.as_str()?.to_string();
            return Some((agent, path));
        }
    }
    None
}
