//! Video-Player auf der Object Page.
//!
//! * `GET /ext/video/stream/{video_id}`: same-origin Proxy. Ermittelt Agent-ID und
//!   `file_path` des Videos aus dem Store und holt die Datei vom Upstream:
//!   `{VIDEO_STREAM_BASE_URL}/{agent}/stream/{base64(file_path)}`. `Range`-Anfragen
//!   werden durchgereicht, damit der Browser spulen kann.
//! Fragment, Controller und Modul des Players liegen als Assets unter
//! `ext/videos/` und werden ueber `ddic.json` (`ui`-Block) eingebunden.
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
use serde_json::Value;
use uuid::Uuid;

use crate::thumbnails::DEFAULT_UPSTREAM;

const VIDEO_SET: &str = "VideoSet";
const FILE_SET: &str = "FileSet";
const VOLUME_SET: &str = "VolumeSet";

pub struct VideoPlayer {
    upstream: String,
}

/// Basis-URL des Video-Streams (`VIDEO_STREAM_BASE_URL`).
pub fn stream_upstream() -> String {
    std::env::var("VIDEO_STREAM_BASE_URL").unwrap_or_else(|_| DEFAULT_UPSTREAM.to_string())
}

impl VideoPlayer {
    /// Upstream aus `VIDEO_STREAM_BASE_URL`, Default wie beim Thumbnail-Dienst.
    pub fn from_env() -> Self {
        Self {
            upstream: stream_upstream(),
        }
    }
}

struct PlayerState {
    app: AppState,
    client: reqwest::Client,
    upstream: String,
}

impl Extension for VideoPlayer {
    fn status(&self) -> Option<(String, Value)> {
        Some((
            "video.player".into(),
            crate::status::stream_json(&self.upstream),
        ))
    }

    fn routes(&self, app: AppState) -> Router {
        let state = Arc::new(PlayerState {
            app,
            client: reqwest::Client::new(),
            upstream: self.upstream.clone(),
        });
        Router::new()
            .route("/ext/video/stream/{video_id}", get(stream))
            .with_state(state)
    }
}


async fn stream(
    state: State<Arc<PlayerState>>,
    path: Path<String>,
    req_headers: HeaderMap,
) -> Result<Response, StatusCode> {
    let res = stream_inner(state, path, req_headers).await;
    let code = match &res {
        Ok(r) => r.status().as_u16(),
        Err(s) => s.as_u16(),
    };
    crate::status::stream_result(code);
    res
}

async fn stream_inner(
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

/// Agent-ID und absoluter Dateipfad des Videos. Liest zuerst die aktive Version,
/// sonst den Draft. Der Pfad ergibt sich aus Volume-Mount und relativem Pfad der
/// ersten Datei des Videos (Trennzeichen je nach Mount: `\` bei Laufwerken).
async fn source_of(app: &AppState, id: Uuid) -> Option<(String, String)> {
    for active in [true, false] {
        let key = EntityKey::new(id, active);
        let Ok(record) = app
            .store
            .read_entity(VIDEO_SET, &key, &ODataQuery::default())
            .await
        else {
            continue;
        };
        let agent = record.get("agent_ID")?.as_str()?.to_string();
        let files = app
            .store
            .get_collection(FILE_SET, &ODataQuery::new().filter(video_filter(id)))
            .await
            .ok()?;
        let file = files.iter().find(|f| f.is_active_entity() == active)?;
        let relative = file.get("relative_path")?.as_str()?.to_string();
        let volume_id = Uuid::parse_str(file.get("volume_ID")?.as_str()?).ok()?;
        let volume = app
            .store
            .read_entity(VOLUME_SET, &EntityKey::new(volume_id, true), &ODataQuery::default())
            .await
            .ok()?;
        let mount = volume.get("mount_path")?.as_str()?.to_string();
        return Some((agent, absolute_path(&mount, &relative)));
    }
    None
}

/// `video_ID eq <id>` als OData-Filter.
fn video_filter(id: Uuid) -> odata::query::Expr {
    use odata::query::{BinOp, Expr};
    Expr::BinaryOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Identifier("video_ID".into())),
        right: Box::new(Expr::Guid(id.to_string())),
    }
}

/// Mount plus relativer Pfad. Laufwerke (`F:\`) bekommen Backslashes, Unix-Mounts `/`.
fn absolute_path(mount: &str, relative: &str) -> String {
    if mount.ends_with(":\\") {
        format!("{}{}", mount, relative.replace('/', "\\"))
    } else {
        format!("{}/{}", mount.trim_end_matches('/'), relative)
    }
}

#[cfg(test)]
mod path_tests {
    use super::absolute_path;

    #[test]
    fn unix_mount_joins_with_slash() {
        assert_eq!(absolute_path("/mnt/33/peter", "cb/a.mp4"), "/mnt/33/peter/cb/a.mp4");
    }

    #[test]
    fn drive_mount_uses_backslashes() {
        assert_eq!(absolute_path("F:\\", "d/chaturbate/x.mp4"), "F:\\d\\chaturbate\\x.mp4");
    }
}
