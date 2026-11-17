use std::sync::Arc;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use data_store::{DataStore, DraftDataStore, StaticSchema};
use fiori_server::{
    AppState,
    config::{ServerConfig, ServerMode},
    script::ScriptExtension,
    srv,
};
use script_hooks::ScriptHooks;
use serde_json::{Value, json};
use spec::data_dictionary::DataDictionary;
use sqlite_relational_store::RelationalStore;
use tempfile::TempDir;
use tower::ServiceExt;

fn dictionary() -> DataDictionary {
    serde_json::from_value(json!({
        "packages": [{"name": "test"}],
        "entities": [{
            "name": "Order", "package": "test", "type_name_plural": "Orders",
            "fields": [
                {"Atom": {"name": "name", "label": "Name", "edm_type": "Edm.String"}},
                {"Atom": {"name": "status", "label": "Status", "edm_type": "Edm.String"}}
            ],
            "actions": [
                {"name": "start", "label": "Start & prüfen", "list": true, "header": true},
                {"name": "bad", "label": "Bad"}
            ]
        }],
        "relationships": [], "apps": []
    }))
    .unwrap()
}

async fn request(
    state: &AppState,
    method: Method,
    path: &str,
    body: Value,
) -> (StatusCode, String) {
    let response = srv::router()
        .with_state(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
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

#[tokio::test]
async fn actions_are_advertised_and_apply_only_valid_patches() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("OrderSet.rhai"),
        r#"
        fn action_start(ctx) {
            if ctx.record.status != "planned" { throw "already started"; }
            ctx.patch.status = "started";
            ctx
        }
        fn action_bad(ctx) { ctx.patch.ID = "other"; ctx }
    "#,
    )
    .unwrap();
    let ddic = dictionary();
    let extension = ScriptExtension::new(ScriptHooks::load(dir.path()).unwrap())
        .with_actions(&ddic)
        .unwrap();
    let mut active = ddic.clone();
    active.activate();
    let schema = spec_schema::schema_from_ddic(&active).unwrap();
    let store = Arc::new(
        RelationalStore::open_in_memory(Arc::new(StaticSchema::new(schema)))
            .await
            .unwrap(),
    );
    let created = store
        .create_entity("OrderSet", &json!({"name":"A", "status":"planned"}))
        .await
        .unwrap();
    let key = created.key();
    store.draft_activate("OrderSet", &key).await.unwrap();
    let state = AppState::new(
        store.clone(),
        Arc::new(ServerConfig::defaults(ServerMode::Runtime)),
        ddic,
    )
    .with_extensions(vec![Arc::new(extension)]);
    let (status, metadata) = request(&state, Method::GET, "/$metadata", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert!(metadata.contains("<Action Name=\"start\""));
    assert!(metadata.contains("UI.DataFieldForAction"));
    assert!(metadata.contains("Start &amp; prüfen"));
    let uri = format!("/OrderSet(ID={},IsActiveEntity=true)/Service.start", key.id);
    let (status, body) = request(&state, Method::POST, &uri, json!({"unexpected":1})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, body) = request(&state, Method::POST, &uri, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["status"],
        "started"
    );
    let (status, body) = request(&state, Method::POST, &uri, json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("already started"));
    let (status, _) = request(
        &state,
        Method::POST,
        &format!("/OrderSet(ID={},IsActiveEntity=true)/Service.bad", key.id),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = request(
        &state,
        Method::POST,
        &format!(
            "/OrderSet(ID={},IsActiveEntity=false)/Service.start",
            key.id
        ),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    store
        .draft_edit(
            "OrderSet",
            &data_store::EntityKey::new(key.id.clone(), true),
        )
        .await
        .unwrap();
    let (status, body) = request(&state, Method::POST, &uri, json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("offenen Entwurf"), "{body}");
}

#[test]
fn missing_action_function_is_rejected() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("OrderSet.rhai"),
        "fn action_start(ctx) { ctx }",
    )
    .unwrap();
    let result =
        ScriptExtension::new(ScriptHooks::load(dir.path()).unwrap()).with_actions(&dictionary());
    assert!(result.err().unwrap().to_string().contains("action_bad"));
}

#[tokio::test]
async fn action_on_navigation_uses_the_linked_child() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("ChildSet.rhai"),
        "fn action_start(ctx) { ctx.patch.status = \"started\"; ctx }",
    )
    .unwrap();
    let ddic: DataDictionary = serde_json::from_value(json!({
        "packages": [{"name": "test"}],
        "entities": [
            {"name": "Parent", "package": "test", "type_name_plural": "Parents",
             "fields": [{"Atom": {"name": "name", "label": "Name", "edm_type": "Edm.String"}}]},
            {"name": "Child", "package": "test", "type_name_plural": "Children",
             "fields": [{"Atom": {"name": "status", "label": "Status", "edm_type": "Edm.String"}}],
             "actions": [{"name": "start", "label": "Start", "header": true}]}
        ],
        "relationships": [{"name": "children", "one": "Parent", "one_field": "_Children",
            "many": "Child", "many_property": "parent", "package": "test"}],
        "apps": []
    }))
    .unwrap();
    let extension = ScriptExtension::new(ScriptHooks::load(dir.path()).unwrap())
        .with_actions(&ddic)
        .unwrap();
    let mut active = ddic.clone();
    active.activate();
    let schema = spec_schema::schema_from_ddic(&active).unwrap();
    let store = Arc::new(
        RelationalStore::open_in_memory(Arc::new(StaticSchema::new(schema)))
            .await
            .unwrap(),
    );
    let parent = store
        .create_entity("ParentSet", &json!({"name": "P"}))
        .await
        .unwrap();
    store
        .draft_activate("ParentSet", &parent.key())
        .await
        .unwrap();
    let other = store
        .create_entity("ParentSet", &json!({"name": "Other"}))
        .await
        .unwrap();
    store
        .draft_activate("ParentSet", &other.key())
        .await
        .unwrap();
    let child = store
        .create_entity(
            "ChildSet",
            &json!({"status": "planned", "parent_ID": parent.id().to_string()}),
        )
        .await
        .unwrap();
    store
        .draft_activate("ChildSet", &child.key())
        .await
        .unwrap();
    let state = AppState::new(
        store,
        Arc::new(ServerConfig::defaults(ServerMode::Runtime)),
        ddic,
    )
    .with_extensions(vec![Arc::new(extension)]);
    let uri = |id| {
        format!(
            "/ParentSet(ID={id},IsActiveEntity=true)/_Children(ID={},IsActiveEntity=true)/Service.start",
            child.id()
        )
    };
    let (status, _) = request(&state, Method::POST, &uri(other.id()), json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body) = request(&state, Method::POST, &uri(parent.id()), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["status"],
        "started"
    );
}
