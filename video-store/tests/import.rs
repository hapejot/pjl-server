mod common;

use data_store::DataStore;
use odata::query::ODataQuery;
use serde_json::{Map, Value, json};
use sqlite_relational_store::RelationalStore;
use std::sync::Arc;
use video_store::{import, map};

const AGENT: &str = "11111111-1111-4111-8111-111111111111";
const VIDEO: &str = "22222222-2222-4222-8222-222222222222";
const TAG: &str = "33333333-3333-4333-8333-333333333333";

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn model() -> map::Model {
    map::build_model(&common::catalog()).unwrap()
}

#[test]
fn uuid_pk_is_kept_and_fk_column_is_remapped() {
    let m = model();
    let rows = vec![obj(json!({"id": VIDEO, "agent_id": AGENT, "title": "x",
        "file_size": 7, "duration_seconds": 12.5, "scene_id": null}))];
    let records = import::records_for_table(&m, &m.tables["videos"], &rows).unwrap();
    let r = &records[0];
    assert_eq!(r["ID"], VIDEO);
    assert_eq!(r["IsActiveEntity"], true);
    assert_eq!(r["agent_ID"], AGENT);
    assert_eq!(r["duration_seconds"], 12.5);
    assert_eq!(r["scene_ID"], Value::Null);
    assert!(r.get("agent_id").is_none(), "Quell-FK darf nicht als Atom bleiben");
    assert!(r.get("id").is_none(), "Quell-PK wird nicht als Atom gefuehrt");
}

#[test]
fn text_key_gets_derived_id_and_legacy_id_and_consistent_references() {
    let m = model();
    let scenes = records_of(&m, "scenes", json!([{"id": "scene-7", "title": "t"}]));
    let legacy = scenes[0]["legacy_id"].clone();
    assert_eq!(legacy, "scene-7");
    let derived = scenes[0]["ID"].as_str().unwrap().to_string();
    assert_ne!(derived, "scene-7");

    // Ein Verweis auf dieselbe Szene muss dieselbe ID ergeben.
    let videos = records_of(
        &m,
        "videos",
        json!([{"id": VIDEO, "agent_id": AGENT, "file_size": 1, "scene_id": "scene-7"}]),
    );
    assert_eq!(videos[0]["scene_ID"], derived);
}

#[test]
fn derived_ids_are_deterministic() {
    let m = model();
    let a = records_of(&m, "faces", json!([{"id": 42, "name": "a"}]));
    let b = records_of(&m, "faces", json!([{"id": 42, "name": "a"}]));
    assert_eq!(a[0]["ID"], b[0]["ID"]);
    assert_eq!(a[0]["legacy_id"], "42");
}

#[test]
fn bytea_hex_becomes_base64() {
    let m = model();
    let r = records_of(&m, "faces", json!([{"id": 1, "image_data": "\\x0102ff"}]));
    assert_eq!(r[0]["image_data"], "AQL/");
}

#[test]
fn pk_less_duplicate_rows_get_distinct_ids() {
    let m = model();
    let r = records_of(
        &m,
        "actor_scene_map",
        json!([{"actor_id": "a", "scene_id": "s"}, {"actor_id": "a", "scene_id": "s"}]),
    );
    assert_ne!(r[0]["ID"], r[1]["ID"]);
}

#[test]
fn label_category_fk_targets_the_non_uuid_key() {
    let m = model();
    let cat = records_of(&m, "label_categories", json!([{"key": "color"}]));
    let opt = records_of(
        &m,
        "label_category_options",
        json!([{"id": VIDEO, "category_key": "color", "value": "red"}]),
    );
    assert_eq!(opt[0]["category_ID"], cat[0]["ID"]);
}

#[tokio::test]
async fn full_write_in_parent_first_order_into_sqlite() {
    let m = model();
    let dir = tempfile::tempdir().unwrap();
    let source = spec_schema::DdicSchemaSource::new(&m.activated()).unwrap();
    let store = RelationalStore::open(dir.path().join("db"), Arc::new(source))
        .await
        .unwrap();

    let fixtures: Vec<(&str, Value)> = vec![
        ("agents", json!([{"id": AGENT, "name": "a", "status": "online"}])),
        ("tags", json!([{"id": TAG, "name": "action"}])),
        ("videos", json!([{"id": VIDEO, "agent_id": AGENT, "file_size": 9, "scene_id": "s-1"}])),
        ("video_tags", json!([{"id": "44444444-4444-4444-8444-444444444444",
            "video_id": VIDEO, "tag_id": TAG, "source": "path"}])),
        ("video_jobs", json!([{"id": "55555555-5555-4555-8555-555555555555",
            "video_id": null, "agent_id": AGENT, "parameters": {"a": 1}}])),
        ("scenes", json!([{"id": "s-1", "title": "scene"}])),
        ("faces", json!([{"id": 3, "name": "n", "image_data": "\\x00ff"}])),
    ];
    let mut expected_video_tags = 0;
    let order = store_order(&m);
    for table in order {
        let set = m.tables.get(&table).unwrap();
        let rows: Vec<Map<String, Value>> = fixtures
            .iter()
            .filter(|(t, _)| *t == table)
            .flat_map(|(_, v)| v.as_array().unwrap().iter().map(|x| obj(x.clone())))
            .collect();
        if table == "video_tags" {
            expected_video_tags = rows.len();
        }
        import::write_rows(&store, &m, set, &rows).await.unwrap();
    }

    // Hinweis: get_collection ohne Filter liefert im aktuellen HEAD zusaetzlich
    // Phantomzeilen (FULL OUTER JOIN in relational-core, shadow_predicate
    // auskommentiert). Deshalb hier immer nach IsActiveEntity filtern.
    let active = active_only();
    let tags = store.get_collection("VideoTagSet", &active).await.unwrap();
    assert_eq!(tags.len(), expected_video_tags);
    let videos = store.get_collection("VideoSet", &active).await.unwrap();
    assert_eq!(videos.len(), 1);
    assert_eq!(videos[0].core_json()["ID"], VIDEO);
    let scenes = store.get_collection("SceneSet", &active).await.unwrap();
    assert_eq!(scenes.len(), 1);
    assert_eq!(scenes[0].core_json()["legacy_id"], "s-1");

}

fn active_only() -> ODataQuery {
    ODataQuery {
        filter: Some(odata::query::Expr::parse("IsActiveEntity eq true").unwrap()),
        ..ODataQuery::default()
    }
}

fn records_of(m: &map::Model, table: &str, rows: Value) -> Vec<Value> {
    let rows: Vec<Map<String, Value>> = rows.as_array().unwrap().iter().map(|x| obj(x.clone())).collect();
    import::records_for_table(m, &m.tables[table], &rows).unwrap()
}

/// Schreibreihenfolge wie im Import: Parents zuerst.
fn store_order(m: &map::Model) -> Vec<String> {
    let schema = m.schema().unwrap();
    schema
        .creation_order()
        .unwrap()
        .into_iter()
        .filter_map(|s| m.table_for_set(&s.name).map(|t| t.table.clone()))
        .collect()
}
