//! Pilot: Festwertliste mit Code als Primaerschluessel (`ProjectStatus`).
//!
//! Prueft Metadaten, Anlage, Zugriff ueber den Code im Pfad, Navigation auf die
//! Liste und Pfadausdruecke (`status/name`), sowie die Unveraenderlichkeit des Schluessels.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use data_store::StaticSchema;
use fiori_server::{
    AppState,
    config::{ServerConfig, ServerMode},
    srv,
};
use serde_json::{Value, json};
use spec::{
    AtomValueList, EntitySpec, FieldSpec, ValueList, data_dictionary::DataDictionary,
    value_list::ValueEntry,
};
use sqlite_relational_store::RelationalStore;
use tower::ServiceExt;

const LIST_ID: &str = "79714635-4b3e-51a6-be78-dc009beb5f3c";

async fn request(state: &AppState, method: Method, uri: &str, body: Value) -> (StatusCode, String) {
    let response = srv::router()
        .with_state(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

async fn json_request(
    state: &AppState,
    method: Method,
    uri: &str,
    body: Value,
    expected: StatusCode,
) -> Value {
    let (status, text) = request(state, method, uri, body).await;
    assert_eq!(status, expected, "{uri}: {text}");
    if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap()
    }
}

/// Dictionary wie im Designer-Export: Projekt mit Wertehilfe auf eine Code-Liste.
fn base_dictionary() -> DataDictionary {
    let mut ddic = DataDictionary::empty();
    ddic.add_value_list(
        ValueList::new(
            LIST_ID,
            "ProjectStatus",
            vec![
                ValueEntry::new("planned", "Geplant", 1),
                ValueEntry::new("active", "Aktiv", 2),
            ],
        )
        .with_code_key(),
    );
    ddic.add_entity(
        EntitySpec::new("Project", "test", "Projects")
            .without_drafts()
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(
                FieldSpec::atom("status", "Status", "Edm.String").with_value_list(
                    AtomValueList::FieldValueList {
                        list_id: LIST_ID.into(),
                        prefer_dialog: false,
                    },
                ),
            ),
    );
    ddic.apply_value_list_lookups().unwrap();
    ddic
}

async fn runtime() -> AppState {
    let base = base_dictionary();
    let mut active = base.clone();
    active.activate();
    let schema = spec_schema::schema_from_ddic(&active).unwrap();
    let store = Arc::new(
        RelationalStore::open_in_memory(Arc::new(StaticSchema::new(schema)))
            .await
            .unwrap(),
    );
    fiori_server::server::seed_value_lists(store.as_ref(), &active)
        .await
        .unwrap();
    AppState::new(
        store,
        Arc::new(ServerConfig::defaults(ServerMode::Runtime)),
        base,
    )
}

#[tokio::test]
async fn metadata_uses_code_as_key_and_foreign_key_is_the_field() {
    let state = runtime().await;
    let (status, xml) = request(&state, Method::GET, "/$metadata", Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{xml}");
    let list = xml
        .split("<EntityType Name=\"ProjectStatus\">")
        .nth(1)
        .unwrap()
        .split("</EntityType>")
        .next()
        .unwrap();
    assert!(list.contains("<PropertyRef Name=\"value\"/>"), "{list}");
    assert!(!list.contains("Name=\"ID\""), "{list}");
    let project = xml
        .split("<EntityType Name=\"Project\">")
        .nth(1)
        .unwrap()
        .split("</EntityType>")
        .next()
        .unwrap();
    assert!(
        project.contains("Name=\"status_code\" Type=\"Edm.String\""),
        "{project}"
    );
    assert!(
        project.contains("ReferencedProperty=\"value\""),
        "{project}"
    );
}

#[tokio::test]
async fn code_is_key_in_path_and_create() {
    let state = runtime().await;
    let created = json_request(
        &state,
        Method::POST,
        "/ProjectStatusSet",
        json!({"value": "done", "name": "Erledigt"}),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(created["value"], json!("done"));
    assert!(created.get("ID").is_none(), "{created}");

    let read = json_request(
        &state,
        Method::GET,
        "/ProjectStatusSet('done')",
        Value::Null,
        StatusCode::OK,
    )
    .await;
    assert_eq!(read["name"], json!("Erledigt"));
}

#[tokio::test]
async fn project_references_code_and_path_expressions_work() {
    let state = runtime().await;
    let project = json_request(
        &state,
        Method::POST,
        "/ProjectSet",
        json!({"name": "Alpha", "status_code": "planned"}),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(project["status_code"], json!("planned"));
    assert!(project.get("status_ID").is_none(), "{project}");

    let expanded = json_request(
        &state,
        Method::GET,
        "/ProjectSet?$expand=status",
        Value::Null,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        expanded["value"][0]["status"]["name"],
        json!("Geplant"),
        "{expanded}"
    );

    let selected = json_request(
        &state,
        Method::GET,
        "/ProjectSet?$select=name,status&$expand=status($select=name)",
        Value::Null,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        selected["value"][0]["status"]["name"],
        json!("Geplant"),
        "{selected}"
    );
}

#[tokio::test]
async fn key_cannot_be_changed_by_patch() {
    let state = runtime().await;
    let (status, text) = request(
        &state,
        Method::PATCH,
        "/ProjectStatusSet('planned')",
        json!({"value": "other", "name": "X"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{text}");
}
