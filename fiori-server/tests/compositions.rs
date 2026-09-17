use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use fiori_server::{AppState, srv};
use odata_yaml_store::{DataStore, EntityKey, YamlStore};
use serde_json::{Value, json};
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;

const ENTITY_ID: &str = "00000000-0000-0000-0000-000000000001";

fn app_state() -> (TempDir, AppState) {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(YamlStore::open(dir.path()).unwrap());
    store
        .create_entity(
            "EntitySet",
            &json!({ "ID": ENTITY_ID, "IsActiveEntity": true, "name": "Order", "plural": "Orders" }),
        )
        .unwrap();
    let state = AppState::designer(store);
    state.reload();
    (dir, state)
}

async fn request(state: &AppState, method: Method, uri: &str, body: Value) -> (StatusCode, Value) {
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
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

fn fields_of(state: &AppState, parent: &str) -> Vec<Value> {
    let query = odata::query::ODataQuery::new()
        .filter(odata::query::Expr::equals_guid(parent, "entity_ID"));
    state
        .store
        .get_collection("FieldSet", &query)
        .unwrap()
        .iter()
        .map(|r| json!(r))
        .collect()
}

#[tokio::test]
async fn creates_a_child_through_a_navigation() {
    let (_dir, state) = app_state();
    request(
        &state,
        Method::POST,
        &format!("/EntitySet(ID={ENTITY_ID},IsActiveEntity=true)/Service.draftEdit"),
        json!({}),
    )
    .await;

    let (status, body) = request(
        &state,
        Method::POST,
        &format!("/EntitySet(ID={ENTITY_ID},IsActiveEntity=false)/_Fields"),
        json!({ "name": "status", "label": "Status" }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["entity_ID"], json!(ENTITY_ID));
    assert_eq!(body["IsActiveEntity"], json!(false));
    assert_eq!(fields_of(&state, ENTITY_ID).len(), 1);
}

#[tokio::test]
async fn rejects_creation_on_an_unknown_navigation() {
    let (_dir, state) = app_state();

    let (status, _) = request(
        &state,
        Method::POST,
        &format!("/EntitySet(ID={ENTITY_ID},IsActiveEntity=true)/gibtsnicht"),
        json!({}),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Draft-Aktionen und das Verwerfen muessen die Kinder einer Composition mitnehmen.
#[tokio::test]
async fn draft_actions_cascade_to_children() {
    let (_dir, state) = app_state();
    state
        .store
        .create_entity(
            "FieldSet",
            &json!({ "IsActiveEntity": true, "entity_ID": ENTITY_ID, "name": "status" }),
        )
        .unwrap();

    let (status, _) = request(
        &state,
        Method::POST,
        &format!("/EntitySet(ID={ENTITY_ID},IsActiveEntity=true)/Service.draftEdit"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let drafted = fields_of(&state, ENTITY_ID);
    assert_eq!(drafted.len(), 1);
    assert_eq!(drafted[0]["IsActiveEntity"], json!(false));

    let (status, _) = request(
        &state,
        Method::POST,
        &format!("/EntitySet(ID={ENTITY_ID},IsActiveEntity=false)/Service.draftActivate"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let activated = fields_of(&state, ENTITY_ID);
    assert_eq!(activated.len(), 1);
    assert_eq!(activated[0]["IsActiveEntity"], json!(true));
}

#[tokio::test]
async fn discarding_a_draft_keeps_the_active_children() {
    let (_dir, state) = app_state();
    state
        .store
        .create_entity(
            "FieldSet",
            &json!({ "IsActiveEntity": true, "entity_ID": ENTITY_ID, "name": "status" }),
        )
        .unwrap();
    request(
        &state,
        Method::POST,
        &format!("/EntitySet(ID={ENTITY_ID},IsActiveEntity=true)/Service.draftEdit"),
        json!({}),
    )
    .await;

    let (status, _) = request(
        &state,
        Method::DELETE,
        &format!("/EntitySet(ID={ENTITY_ID},IsActiveEntity=false)"),
        Value::Null,
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    let remaining = fields_of(&state, ENTITY_ID);
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0]["IsActiveEntity"], json!(true));
    assert!(
        state
            .store
            .read_entity(
                "EntitySet",
                &EntityKey::new_active(ENTITY_ID),
                &Default::default()
            )
            .is_ok()
    );
}

#[tokio::test]
async fn deleting_the_parent_removes_its_children() {
    let (_dir, state) = app_state();
    state
        .store
        .create_entity(
            "FieldSet",
            &json!({ "IsActiveEntity": true, "entity_ID": ENTITY_ID, "name": "status" }),
        )
        .unwrap();

    let (status, _) = request(
        &state,
        Method::DELETE,
        &format!("/EntitySet(ID={ENTITY_ID},IsActiveEntity=true)"),
        Value::Null,
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(fields_of(&state, ENTITY_ID).is_empty());
}

#[tokio::test]
async fn metadata_marks_children_as_draft_nodes() {
    let (_dir, state) = app_state();
    let response = srv::router()
        .with_state(state.clone())
        .oneshot(
            Request::builder()
                .uri("/$metadata")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let xml = String::from_utf8(bytes.to_vec()).unwrap();

    assert!(xml.contains(r#"<EntitySet Name="FieldSet""#));
    assert!(xml.contains(r#"<Annotations Target="Service.EntityContainer/FieldSet">"#));
    assert!(xml.contains(r#"Term="Common.DraftNode""#));
    assert!(xml.contains(r#"NavigationPropertyPath="_Fields""#));
}
