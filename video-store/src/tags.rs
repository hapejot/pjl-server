//! Tag-Facetten und Trefferliste fuer die Video-Oberflaeche. Alle Abfragen laufen
//! direkt als SQL gegen die PostgreSQL-Datenbank, es gibt keinen Speicher-Index.
//!
//! * `GET /ext/video/tags/facets?selected=<id>,<id>&top=20&q=<text>`
//!   liefert `{ "matches": n, "tags": [{ "id", "name", "count" }] }`.
//!   `matches` ist die Anzahl Videos mit allen gewaehlten Tags, `tags` die
//!   uebrigen Tags mit mindestens einem Treffer, absteigend nach Trefferzahl.
//! * `GET /ext/video/tags/videos?selected=..&skip=0&top=100`
//!   liefert `{ "count": n, "value": [{ "ID", "name", "duration_seconds" }] }`,
//!   sortiert nach Name.
//! * Gezaehlt werden nur aktive, sichtbare Tags und aktive, nicht geloeschte Videos.
//! * `400` bei unbekannter Tag-ID, `503` ohne PostgreSQL-Store.
//!
//! Die Ergebnisse ohne Auswahl (Rangliste ueber alle Tags) sind fuer
//! `TAG_CACHE_SECS` Sekunden zwischengespeichert. Mit Auswahl wird immer frisch gerechnet.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use fiori_server::{extension::Extension, AppState};
use postgres_relational_store::sqltrace;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio_postgres::NoTls;
use tracing::error;

const DEFAULT_TOP: usize = 20;
const MAX_TOP: usize = 200;
const DEFAULT_LIST: usize = 100;
const MAX_LIST: usize = 200;
const TAG_CACHE_SECS: u64 = 60;

/// Facette: Tag mit Trefferzahl.
#[derive(Debug, Clone)]
struct Facet {
    id: String,
    name: String,
    count: i64,
}

/// Rangliste ohne Auswahl: Gesamtzahl und alle sichtbaren Tags mit Treffern.
#[derive(Debug)]
struct Ranking {
    total: i64,
    tags: Vec<Facet>,
}

pub struct VideoTagFacets;

impl VideoTagFacets {
    pub fn new() -> Self {
        Self
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
        let pool = match app.config.store_pg_url.as_deref().map(build_pool) {
            Some(Ok(pool)) => Some(pool),
            Some(Err(e)) => {
                error!("tag facets: pool could not be built: {e:#}");
                None
            }
            None => None,
        };
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
                pool,
                ranking: Mutex::new(None),
            }))
    }
}

fn build_pool(url: &str) -> anyhow::Result<Pool> {
    let config: tokio_postgres::Config = url.parse()?;
    let manager = Manager::from_config(
        config,
        NoTls,
        ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        },
    );
    Ok(Pool::builder(manager).max_size(8).build()?)
}

struct FacetState {
    pool: Option<Pool>,
    ranking: Mutex<Option<(Instant, Arc<Ranking>)>>,
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


fn parse_selected(q: &HashMap<String, String>) -> Vec<String> {
    let mut ids: Vec<String> = q
        .get("selected")
        .map(String::as_str)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

fn parse_usize(q: &HashMap<String, String>, key: &str, default: usize, max: usize) -> usize {
    q.get(key)
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default)
        .min(max)
}

fn error_response(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(json!({ "error": msg.into() }))).into_response()
}

fn internal(e: impl std::fmt::Display) -> Response {
    error!("tag facets: {e}");
    error_response(StatusCode::INTERNAL_SERVER_ERROR, "database error")
}

/// Gemeinsame CTE: Videos, die alle gewaehlten Tags haben (aktiv, nicht geloescht).
const MATCHED_CTE: &str = r#"
matched AS (
  SELECT vt."video_ID" AS vid
  FROM "VideoTagSet" vt
  JOIN "VideoSet" v
    ON v."ID" = vt."video_ID" AND v."IsActiveEntity" AND v."deleted_at" IS NULL
  WHERE vt."IsActiveEntity" AND vt."tag_ID" = ANY($1)
  GROUP BY vt."video_ID"
  HAVING COUNT(DISTINCT vt."tag_ID") = $2
)"#;

/// Prueft, dass alle Auswahl-IDs sichtbare aktive Tags sind.
async fn check_selection(
    client: &deadpool_postgres::Object,
    selected: &[String],
) -> Result<(), Response> {
    if selected.is_empty() {
        return Ok(());
    }
    let rows = sqltrace::query(
        client,
        r#"SELECT "ID" FROM "TagSet"
           WHERE "IsActiveEntity" AND "visible" IS NOT FALSE AND "ID" = ANY($1)"#,
        &[&selected],
    )
    .await
    .map_err(internal)?;
    let known: Vec<String> = rows.iter().map(|r| r.get::<_, String>(0)).collect();
    match selected.iter().find(|id| !known.contains(id)) {
        Some(id) => Err(error_response(
            StatusCode::BAD_REQUEST,
            format!("unknown tag {id}"),
        )),
        None => Ok(()),
    }
}

/// Rangliste ohne Auswahl, aus dem Cache wenn frisch genug.
async fn ranking(st: &FacetState, client: &deadpool_postgres::Object) -> Result<Arc<Ranking>, Response> {
    let mut cache = st.ranking.lock().await;
    if let Some((at, r)) = cache.as_ref() {
        if at.elapsed() < Duration::from_secs(TAG_CACHE_SECS) {
            return Ok(r.clone());
        }
    }
    let total: i64 = sqltrace::query_one(
        client,
        r#"SELECT COUNT(*) FROM "VideoSet" WHERE "IsActiveEntity" AND "deleted_at" IS NULL"#,
        &[],
    )
    .await
    .map_err(internal)?
    .get(0);
    let rows = sqltrace::query(
        client,
        r#"SELECT t."ID", t."name", COUNT(*)::bigint
           FROM "VideoTagSet" vt
           JOIN "VideoSet" v
             ON v."ID" = vt."video_ID" AND v."IsActiveEntity" AND v."deleted_at" IS NULL
           JOIN "TagSet" t
             ON t."ID" = vt."tag_ID" AND t."IsActiveEntity" AND t."visible" IS NOT FALSE
           WHERE vt."IsActiveEntity"
           GROUP BY t."ID", t."name""#,
        &[],
    )
    .await
    .map_err(internal)?;
    let mut tags: Vec<Facet> = rows
        .iter()
        .map(|r| Facet {
            id: r.get(0),
            name: r.get(1),
            count: r.get(2),
        })
        .collect();
    tags.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    let result = Arc::new(Ranking { total, tags });
    *cache = Some((Instant::now(), result.clone()));
    Ok(result)
}

/// Trefferliste: `GET /ext/video/tags/videos?selected=..&skip=0&top=100`
async fn videos(State(st): State<Arc<FacetState>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(pool) = st.pool.as_ref() else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "no postgres store");
    };
    let selected = parse_selected(&q);
    let skip = parse_usize(&q, "skip", 0, usize::MAX) as i64;
    let top = parse_usize(&q, "top", DEFAULT_LIST, MAX_LIST).max(1) as i64;
    let client = match pool.get().await {
        Ok(c) => c,
        Err(e) => return internal(e),
    };
    if let Err(resp) = check_selection(&client, &selected).await {
        return resp;
    }

    let (count, rows) = if selected.is_empty() {
        let total = match ranking(&st, &client).await {
            Ok(r) => r.total,
            Err(resp) => return resp,
        };
        let rows = sqltrace::query(
            &client,
            r#"SELECT v."ID", v."name", CAST(NULLIF(v."duration_seconds", '') AS double precision)
               FROM "VideoSet" v
               WHERE v."IsActiveEntity" AND v."deleted_at" IS NULL
               ORDER BY v."name" COLLATE "C", v."ID"
               LIMIT $1 OFFSET $2"#,
            &[&top, &skip],
        )
        .await;
        (total, rows)
    } else {
        let n = selected.len() as i64;
        let count = sqltrace::query_one(
            &client,
            &format!("WITH {MATCHED_CTE} SELECT COUNT(*) FROM matched"),
            &[&selected, &n],
        )
        .await
        .map(|r| r.get::<_, i64>(0));
        let count = match count {
            Ok(c) => c,
            Err(e) => return internal(e),
        };
        let rows = sqltrace::query(
            &client,
            &format!(
                r#"WITH {MATCHED_CTE}
                   SELECT v."ID", v."name", CAST(NULLIF(v."duration_seconds", '') AS double precision)
                   FROM "VideoSet" v JOIN matched m ON m.vid = v."ID"
                   WHERE v."IsActiveEntity"
                   ORDER BY v."name" COLLATE "C", v."ID"
                   LIMIT $3 OFFSET $4"#
            ),
            &[&selected, &n, &top, &skip],
        )
        .await;
        (count, rows)
    };
    let rows = match rows {
        Ok(r) => r,
        Err(e) => return internal(e),
    };
    Json(json!({
        "count": count,
        "value": rows.iter().map(|r| json!({
            "ID": r.get::<_, String>(0),
            "name": r.get::<_, Option<String>>(1),
            "duration_seconds": r.get::<_, Option<f64>>(2),
        })).collect::<Vec<Value>>(),
    }))
    .into_response()
}

/// Facetten: `GET /ext/video/tags/facets?selected=..&top=20&q=..`
async fn facets(State(st): State<Arc<FacetState>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(pool) = st.pool.as_ref() else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "no postgres store");
    };
    let selected = parse_selected(&q);
    let top = parse_usize(&q, "top", DEFAULT_TOP, MAX_TOP).max(1);
    let needle = q.get("q").map(|s| s.trim().to_lowercase()).unwrap_or_default();
    let client = match pool.get().await {
        Ok(c) => c,
        Err(e) => return internal(e),
    };
    if let Err(resp) = check_selection(&client, &selected).await {
        return resp;
    }

    if selected.is_empty() {
        let r = match ranking(&st, &client).await {
            Ok(r) => r,
            Err(resp) => return resp,
        };
        let tags: Vec<Value> = r
            .tags
            .iter()
            .filter(|f| needle.is_empty() || f.name.to_lowercase().contains(&needle))
            .take(top)
            .map(facet_json)
            .collect();
        return Json(json!({ "matches": r.total, "tags": tags })).into_response();
    }

    let n = selected.len() as i64;
    let top_i = top as i64;
    let matches = sqltrace::query_one(
        &client,
        &format!("WITH {MATCHED_CTE} SELECT COUNT(*) FROM matched"),
        &[&selected, &n],
    )
    .await
    .map(|r| r.get::<_, i64>(0));
    let matches = match matches {
        Ok(m) => m,
        Err(e) => return internal(e),
    };
    let rows = sqltrace::query(
        &client,
        &format!(
            r#"WITH {MATCHED_CTE}
               SELECT t."ID", t."name", COUNT(*)::bigint AS c
               FROM matched m
               JOIN "VideoTagSet" vt ON vt."video_ID" = m.vid AND vt."IsActiveEntity"
               JOIN "TagSet" t ON t."ID" = vt."tag_ID" AND t."IsActiveEntity" AND t."visible" IS NOT FALSE
               WHERE NOT (vt."tag_ID" = ANY($1))
                 AND ($3 = '' OR strpos(lower(t."name"), $3) > 0)
               GROUP BY t."ID", t."name"
               ORDER BY c DESC, t."name" COLLATE "C"
               LIMIT $4"#
        ),
        &[&selected, &n, &needle, &top_i],
    )
    .await;
    let rows = match rows {
        Ok(r) => r,
        Err(e) => return internal(e),
    };
    let tags: Vec<Value> = rows
        .iter()
        .map(|r| {
            facet_json(&Facet {
                id: r.get(0),
                name: r.get(1),
                count: r.get(2),
            })
        })
        .collect();
    Json(json!({ "matches": matches, "tags": tags })).into_response()
}

fn facet_json(f: &Facet) -> Value {
    json!({ "id": f.id, "name": f.name, "count": f.count })
}
