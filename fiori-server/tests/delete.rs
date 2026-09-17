use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use fiori_server::{AppState, srv};
use odata_yaml_store::{DataStore, EntityKey, YamlStore};
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;

const PACKAGE_ID: &str = "00000000-0000-0000-0000-0000000000a1";
const ENTITY_ID: &str = "00000000-0000-0000-0000-000000000001";

fn app_state() -> (TempDir, AppState) {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(YamlStore::open(dir.path()).unwrap());
    store
        .create_entity(
            "PackageSet",
            &json!({ "ID": PACKAGE_ID, "IsActiveEntity": true, "name": "core" }),
        )
        .unwrap();
    store
        .create_entity(
            "EntitySet",
            &json!({ "ID": ENTITY_ID, "IsActiveEntity": true,
                     "name": "Order", "plural": "Orders", "package_ID": PACKAGE_ID }),
        )
        .unwrap();
    let state = AppState::new(store);
    state.reload();
    (dir, state)
}

async fn delete(state: &AppState, uri: &str) -> StatusCode {
    srv::router()
        .with_state(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

fn active_key() -> EntityKey {
    EntityKey::new_active(ENTITY_ID)
}

#[tokio::test]
async fn deletes_an_active_entity() {
    let (_dir, state) = app_state();

    let status = delete(
        &state,
        &format!("/EntitySet(ID={ENTITY_ID},IsActiveEntity=true)"),
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(
        state
            .store
            .read_entity("EntitySet", &active_key(), &Default::default())
            .is_err()
    );
}

#[tokio::test]
async fn deleting_the_active_entity_removes_its_draft() {
    let (_dir, state) = app_state();
    state.store.draft_edit("EntitySet", &active_key()).unwrap();

    let status = delete(
        &state,
        &format!("/EntitySet(ID={ENTITY_ID},IsActiveEntity=true)"),
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(
        state
            .store
            .read_entity("EntitySet", &active_key().sibling(), &Default::default())
            .is_err()
    );
}

#[tokio::test]
async fn deleting_a_draft_keeps_the_active_entity() {
    let (_dir, state) = app_state();
    state.store.draft_edit("EntitySet", &active_key()).unwrap();

    let status = delete(
        &state,
        &format!("/EntitySet(ID={ENTITY_ID},IsActiveEntity=false)"),
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(
        state
            .store
            .read_entity("EntitySet", &active_key(), &Default::default())
            .is_ok()
    );
}

#[tokio::test]
async fn deletes_through_a_navigation_path() {
    let (_dir, state) = app_state();

    let status = delete(
        &state,
        &format!(
            "/PackageSet(ID={PACKAGE_ID},IsActiveEntity=true)/_Entities(ID={ENTITY_ID},IsActiveEntity=true)"
        ),
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(
        state
            .store
            .read_entity("EntitySet", &active_key(), &Default::default())
            .is_err()
    );
}

#[tokio::test]
async fn rejects_delete_without_key() {
    let (_dir, state) = app_state();
    assert_eq!(delete(&state, "/EntitySet").await, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn reports_unknown_entities() {
    let (_dir, state) = app_state();
    let status = delete(
        &state,
        "/EntitySet(ID=00000000-0000-0000-0000-0000000000ff,IsActiveEntity=true)",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
