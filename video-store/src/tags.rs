//! Tag-Facetten-Endpunkt fuer die Video-Oberflaeche.
//!
//! * `GET /ext/video/tags/facets?selected=<id>,<id>&top=20&q=<text>`
//!   `q` filtert die angebotenen Tags nach Name (Teilstring, ohne Gross-/Kleinschreibung).
//!   liefert `{ "matches": n, "tags": [{ "id", "name", "count" }] }`.
//!   `matches` ist die Anzahl Videos mit allen gewaehlten Tags, `tags` die noch
//!   sinnvoll waehlbaren Tags (mindestens ein Treffer), absteigend nach Trefferzahl.
//! * `503` solange der Index noch geladen wird, `400` bei unbekannter Tag-ID.
//!
//! Der Index (`crate::tag_index`) wird beim Start einmal aus dem Store aufgebaut.
//! Aenderungen an Tag-Zuordnungen werden erst nach einem Neustart sichtbar.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use data_store::DataRecord;
use fiori_server::{extension::Extension, AppState};
use odata::query::ODataQuery;
use serde_json::{json, Value};
use tokio::sync::OnceCell;
use tracing::{error, info};

use crate::tag_index::{FacetError, TagIndex};

const DEFAULT_TOP: usize = 20;
const MAX_TOP: usize = 200;
const DEFAULT_LIST: usize = 100;
const MAX_LIST: usize = 200;

pub struct VideoTagFacets {
    index: Arc<OnceCell<Arc<TagIndex>>>,
}

impl VideoTagFacets {
    pub fn new() -> Self {
        Self {
            index: Arc::new(OnceCell::new()),
        }
    }
}

impl Default for VideoTagFacets {
    fn default() -> Self {
        Self::new()
    }
}

impl Extension for VideoTagFacets {
    fn ui5_modules(&self) -> Vec<String> {
        vec!["video/ext/TagSearch".into()]
    }

    fn controller_extensions(&self) -> Vec<(String, String)> {
        vec![(
            "sap.fe.templates.ListReport.ListReportController".into(),
            "video.ext.TagSearchController".into(),
        )]
    }

    fn list_views(&self, entity: &str) -> Vec<Value> {
        if entity != "Video" {
            return Vec::new();
        }
        vec![json!({
            "key": "TagSearch",
            "label": "Tag-Suche",
            "template": "video.ext.fragment.TagSearch"
        })]
    }

    fn routes(&self, app: AppState) -> Router {
        // Index im Hintergrund laden; der Server ist sofort erreichbar.
        let cell = self.index.clone();
        let store_app = app.clone();
        tokio::spawn(async move {
            info!("loading tag index");
            match load_index(&store_app).await {
                Ok(idx) => {
                    info!(
                        videos = idx.video_count(),
                        tags = idx.tag_count(),
                        "tag index ready"
                    );
                    let _ = cell.set(Arc::new(idx));
                }
                Err(e) => error!("tag index could not be built: {e:#}"),
            }
            info!("tag index completed");
        });

        Router::new()
            .route("/ext/video/tags/facets", get(facets))
            .route("/ext/video/tags/videos", get(videos))
            .route("/ext/video/tagsearch", get(search_page))
            .route("/ext/video/tagsearch/", get(search_page))
            .route("/ext/video/tagsearch/index.html", get(search_page))
            .route("/ext/video/tagsearch/App.js", get(search_app))
            .route("/ext/video/fragment/TagSearch.fragment.xml", get(list_fragment))
            .route("/ext/video/TagSearch.js", get(list_module))
            .route(
                "/ext/video/TagSearchController.controller.js",
                get(list_controller),
            )
            .with_state(Arc::new(FacetState {
                index: self.index.clone(),
            }))
    }
}

const SEARCH_PAGE: &str = include_str!("../tagsearch/index.html");
const SEARCH_APP: &str = include_str!("../tagsearch/App.js");
const LIST_FRAGMENT: &str = include_str!("../fragments/TagSearch.fragment.xml");
const LIST_MODULE: &str = include_str!("../ui5/TagSearch.js");
const LIST_CONTROLLER: &str = include_str!("../ui5/TagSearchController.js");

async fn list_controller() -> Response {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        LIST_CONTROLLER,
    )
        .into_response()
}

async fn list_fragment() -> Response {
    (
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        LIST_FRAGMENT,
    )
        .into_response()
}

async fn list_module() -> Response {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        LIST_MODULE,
    )
        .into_response()
}

async fn search_page() -> Response {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        SEARCH_PAGE,
    )
        .into_response()
}

async fn search_app() -> Response {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        SEARCH_APP,
    )
        .into_response()
}

struct FacetState {
    index: Arc<OnceCell<Arc<TagIndex>>>,
}

fn parse_selected(q: &HashMap<String, String>) -> Vec<String> {
    q.get("selected")
        .map(String::as_str)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_usize(q: &HashMap<String, String>, key: &str, default: usize, max: usize) -> usize {
    q.get(key)
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default)
        .min(max)
}

fn not_ready() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": "tag index is loading" })),
    )
        .into_response()
}

fn unknown_tag(id: String) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": format!("unknown tag {id}") })),
    )
        .into_response()
}

/// Trefferliste: `GET /ext/video/tags/videos?selected=..&skip=0&top=100`
/// Liefert `{ "count": n, "value": [{ "ID", "name", "duration_seconds" }] }`, sortiert nach Name.
async fn videos(
    State(st): State<Arc<FacetState>>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let Some(index) = st.index.get() else {
        return not_ready();
    };
    let selected = parse_selected(&q);
    let skip = parse_usize(&q, "skip", 0, usize::MAX);
    let top = parse_usize(&q, "top", DEFAULT_LIST, MAX_LIST).max(1);

    let index = index.clone();
    let result = tokio::task::spawn_blocking(move || index.videos(&selected, skip, top)).await;
    match result {
        Ok(Ok((count, rows))) => Json(json!({
            "count": count,
            "value": rows.iter().map(|r| json!({
                "ID": r.id,
                "name": r.name,
                "duration_seconds": r.duration_seconds,
            })).collect::<Vec<Value>>(),
        }))
        .into_response(),
        Ok(Err(FacetError::UnknownTag(id))) => unknown_tag(id),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn facets(
    State(st): State<Arc<FacetState>>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let Some(index) = st.index.get() else {
        return not_ready();
    };
    let selected = parse_selected(&q);
    let top = parse_usize(&q, "top", DEFAULT_TOP, MAX_TOP).max(1);
    let needle = q.get("q").cloned().unwrap_or_default();

    // Die Berechnung ist CPU-lastig; ausserhalb des Async-Threads ausfuehren.
    let index = index.clone();
    let result =
        tokio::task::spawn_blocking(move || index.facets_matching(&selected, top, &needle)).await;
    match result {
        Ok(Ok(r)) => Json(json!({
            "matches": r.matches,
            "tags": r.tags.iter().map(|f| json!({
                "id": f.id, "name": f.name, "count": f.count
            })).collect::<Vec<Value>>(),
        }))
        .into_response(),
        Ok(Err(FacetError::UnknownTag(id))) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("unknown tag {id}") })),
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Index aus Tags, Zuordnungen und Videos des Stores aufbauen.
async fn load_index(app: &AppState) -> anyhow::Result<TagIndex> {
    let query = ODataQuery::new();
    let tags = app
        .store
        .get_collection("TagSet", &query)
        .await
        .map_err(|e| anyhow::anyhow!("TagSet: {e}"))?;
    let rows = app
        .store
        .get_collection("VideoTagSet", &query)
        .await
        .map_err(|e| anyhow::anyhow!("VideoTagSet: {e}"))?;
    let videos = app
        .store
        .get_collection("VideoSet", &query)
        .await
        .map_err(|e| anyhow::anyhow!("VideoSet: {e}"))?;

    // Der Aufbau kostet einige Sekunden; blockierend im Hintergrund-Thread.
    let index = tokio::task::spawn_blocking(move || {
        let (names, assignments) = index_inputs(&tags, &rows);
        let mut index = TagIndex::build(assignments, &names);
        index.attach_videos(video_meta(&videos));
        index
    })
    .await?;
    Ok(index)
}

/// Video-Metadaten (ID, Name, Dauer) der aktiven Videos.
pub(crate) fn video_meta(videos: &[DataRecord]) -> Vec<(String, String, Option<f64>)> {
    videos
        .iter()
        .filter(|r| r.is_active_entity())
        .filter_map(|r| {
            let id = r.get("ID")?.as_str()?.to_string();
            let name = r
                .get("name")
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            let duration = r.get("duration_seconds").and_then(|v| v.as_f64());
            Some((id, name, duration))
        })
        .collect()
}

/// Eingaben fuer den Index aus Tag- und Zuordnungs-Records.
///
/// Unsichtbare Tags (`visible = false`) fliegen komplett raus: weder als Facette
/// noch als Auswahl. Zuordnungen zu solchen Tags werden ebenfalls verworfen.
/// Fehlt das Feld `visible`, gilt der Tag als sichtbar.
pub(crate) fn index_inputs(
    tags: &[DataRecord],
    rows: &[DataRecord],
) -> (HashMap<String, String>, Vec<(String, String)>) {
    let mut names: HashMap<String, String> = HashMap::new();
    for t in tags.iter().filter(|r| r.is_active_entity()) {
        if t.get("visible").and_then(|v| v.as_bool()) == Some(false) {
            continue;
        }
        if let (Some(id), Some(name)) = (
            t.get("ID").and_then(|v| v.as_str().map(str::to_string)),
            t.get("name").and_then(|v| v.as_str().map(str::to_string)),
        ) {
            names.insert(id, name);
        }
    }
    let assignments = rows
        .iter()
        .filter(|r| r.is_active_entity())
        .filter_map(|r| {
            let video = r.get("video_ID")?.as_str()?.to_string();
            let tag = r.get("tag_ID")?.as_str()?.to_string();
            names.contains_key(&tag).then_some((video, tag))
        })
        .collect();
    (names, assignments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rec(v: Value) -> DataRecord {
        DataRecord::from_json(&v).expect("record")
    }

    #[test]
    fn invisible_tags_and_their_assignments_are_dropped() {
        let tags = vec![
            rec(
                json!({"ID": "11111111-1111-4111-8111-111111111111", "IsActiveEntity": true, "name": "sichtbar", "visible": true}),
            ),
            rec(
                json!({"ID": "22222222-2222-4222-8222-222222222222", "IsActiveEntity": true, "name": "versteckt", "visible": false}),
            ),
            rec(
                json!({"ID": "33333333-3333-4333-8333-333333333333", "IsActiveEntity": true, "name": "ohne Feld"}),
            ),
        ];
        let rows = vec![
            rec(
                json!({"ID": "c1111111-1111-4111-8111-111111111111", "IsActiveEntity": true, "video_ID": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "tag_ID": "11111111-1111-4111-8111-111111111111"}),
            ),
            rec(
                json!({"ID": "c2222222-2222-4222-8222-222222222222", "IsActiveEntity": true, "video_ID": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "tag_ID": "22222222-2222-4222-8222-222222222222"}),
            ),
            rec(
                json!({"ID": "c3333333-3333-4333-8333-333333333333", "IsActiveEntity": true, "video_ID": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "tag_ID": "33333333-3333-4333-8333-333333333333"}),
            ),
        ];
        let (names, assignments) = index_inputs(&tags, &rows);
        assert!(names.contains_key("11111111-1111-4111-8111-111111111111"));
        assert!(!names.contains_key("22222222-2222-4222-8222-222222222222"));
        assert!(
            names.contains_key("33333333-3333-4333-8333-333333333333"),
            "fehlendes visible gilt als sichtbar"
        );
        assert_eq!(
            assignments,
            vec![
                (
                    "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".to_string(),
                    "11111111-1111-4111-8111-111111111111".to_string()
                ),
                (
                    "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".to_string(),
                    "33333333-3333-4333-8333-333333333333".to_string()
                ),
            ]
        );
    }
}
