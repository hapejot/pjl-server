use std::sync::Arc;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use data_store::{DataStore, DraftDataStore, EntityKey, StaticSchema};
use fiori_server::{
    AppState,
    config::{ServerConfig, ServerMode},
    ddic_store::DdicStore,
    srv,
};
use serde_json::{Value, json};
use spec::{EntitySpec, FieldSpec, Relationship, data_dictionary::DataDictionary};
use sqlite_relational_store::RelationalStore;
use tempfile::TempDir;
use tower::ServiceExt;

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

fn assert_no_drafts(value: &Value) {
    match value {
        Value::Array(items) => items.iter().for_each(assert_no_drafts),
        Value::Object(object) => {
            for name in [
                "IsActiveEntity",
                "HasActiveEntity",
                "HasDraftEntity",
                "DraftAdministrativeData",
                "DraftAdministrativeData_ID",
                "DraftMessages",
                "SiblingEntity",
            ] {
                assert!(!object.contains_key(name), "{value}");
            }
            object.values().for_each(assert_no_drafts);
        }
        _ => {}
    }
}

fn entity_type<'a>(xml: &'a str, name: &str) -> &'a str {
    xml.split(&format!("<EntityType Name=\"{name}\">"))
        .nth(1)
        .unwrap()
        .split("</EntityType>")
        .next()
        .unwrap()
}

fn runtime_dictionary() -> DataDictionary {
    let mut ddic = DataDictionary::empty();
    for entity in [
        EntitySpec::new("Catalog", "test", "Catalogs").without_drafts(),
        EntitySpec::new("Entry", "test", "Entries").without_drafts(),
        EntitySpec::new("Order", "test", "Orders"),
    ] {
        ddic.add_entity(entity.field(FieldSpec::atom("name", "Name", "Edm.String")));
    }
    ddic.add_relationship(
        Relationship::new("catalog_entries", "test")
            .with_one("Catalog", "_Entries")
            .with_many("Entry", "catalog")
            .owned(),
    );
    ddic.add_relationship(
        Relationship::new("catalog_orders", "test")
            .with_one("Catalog", "_Orders")
            .with_many("Order", "catalog"),
    );
    ddic
}

async fn runtime() -> AppState {
    let base = runtime_dictionary();
    let mut active = base.clone();
    active.activate();
    let schema = spec_schema::schema_from_ddic(&active).unwrap();
    let store = RelationalStore::open_in_memory(Arc::new(StaticSchema::new(schema)))
        .await
        .unwrap();
    AppState::new(
        Arc::new(store),
        Arc::new(ServerConfig::defaults(ServerMode::Runtime)),
        base,
    )
}

#[tokio::test]
async fn designer_has_no_draft_metadata_and_persists_direct_crud() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ddic.json");
    let store = Arc::new(DdicStore::open(&path).unwrap());
    let state = AppState::designer(store.clone());
    let (status, xml) = request(&state, Method::GET, "/$metadata", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    for forbidden in [
        "IsActiveEntity",
        "HasActiveEntity",
        "HasDraftEntity",
        "SiblingEntity",
        "DraftAdministrativeData",
        "Common.DraftRoot",
        "Common.DraftNode",
        "Name=\"draftEdit\"",
        "Name=\"draftActivate\"",
        "Name=\"draftPrepare\"",
    ] {
        assert!(!xml.contains(forbidden), "{forbidden}");
    }
    assert!(entity_type(&xml, "Entity").contains("<Key><PropertyRef Name=\"ID\"/></Key>"));

    let package = json_request(
        &state,
        Method::POST,
        "/PackageSet",
        json!({"name":"test"}),
        StatusCode::CREATED,
    )
    .await;
    assert_no_drafts(&package);
    let entity = json_request(&state, Method::POST, "/EntitySet",
        json!({"name":"Catalog","plural":"Catalogs","package_ID":package["ID"],"draft_enabled":false}),
        StatusCode::CREATED).await;
    assert_no_drafts(&entity);
    let id = entity["ID"].as_str().unwrap();
    let field = json_request(
        &state,
        Method::POST,
        &format!("/EntitySet({id})/_Fields"),
        json!({"name":"name","label":"Name","type":"Edm.String"}),
        StatusCode::CREATED,
    )
    .await;
    assert_no_drafts(&field);
    json_request(
        &state,
        Method::PATCH,
        &format!("/EntitySet(ID={id})"),
        json!({"plural":"Catalog entries"}),
        StatusCode::NO_CONTENT,
    )
    .await;
    let record = json_request(
        &state,
        Method::GET,
        &format!("/EntitySet({id})"),
        Value::Null,
        StatusCode::OK,
    )
    .await;
    assert_eq!(record["plural"], "Catalog entries");
    assert_no_drafts(&record);
    let fields = json_request(
        &state,
        Method::GET,
        &format!("/EntitySet({id})/_Fields"),
        Value::Null,
        StatusCode::OK,
    )
    .await;
    assert_eq!(fields["value"].as_array().unwrap().len(), 1);
    assert_no_drafts(&fields);
    let disk: DataDictionary =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(
        !disk
            .entities()
            .iter()
            .find(|e| e.name == "Catalog")
            .unwrap()
            .draft_enabled
    );
    let reopened = DdicStore::open(&path).unwrap();
    let saved = reopened
        .get_collection("EntitySet", &Default::default())
        .await
        .unwrap();
    assert_eq!(saved[0].get("draft_enabled"), Some(json!(false)));
    assert_eq!(saved[0].get("plural"), Some(json!("Catalog entries")));
    assert!(
        store
            .draft_edit("EntitySet", &EntityKey::new_active(id))
            .await
            .is_err()
    );

    json_request(
        &state,
        Method::POST,
        &format!("/EntitySet({id})/Service.draftEdit"),
        json!({}),
        StatusCode::NOT_FOUND,
    )
    .await;
    json_request(
        &state,
        Method::GET,
        &format!("/EntitySet(ID={id},IsActiveEntity=false)"),
        Value::Null,
        StatusCode::BAD_REQUEST,
    )
    .await;
    json_request(
        &state,
        Method::GET,
        "/EntitySet?$expand=DraftAdministrativeData",
        Value::Null,
        StatusCode::BAD_REQUEST,
    )
    .await;
    json_request(
        &state,
        Method::DELETE,
        &format!("/EntitySet({id})"),
        Value::Null,
        StatusCode::NO_CONTENT,
    )
    .await;
    let reopened = DdicStore::open(&path).unwrap();
    assert_eq!(reopened.count("EntitySet", &Default::default()).await, 0);
    assert_eq!(reopened.count("FieldSet", &Default::default()).await, 0);
}

#[tokio::test]
async fn runtime_mixes_drafts_and_direct_entities_including_navigation_and_expansion() {
    let state = runtime().await;
    let (_, xml) = request(&state, Method::GET, "/$metadata", Value::Null).await;
    let catalog_type = entity_type(&xml, "Catalog");
    assert!(catalog_type.contains("<Key><PropertyRef Name=\"ID\"/></Key>"));
    assert!(!catalog_type.contains("IsActiveEntity"));
    assert!(entity_type(&xml, "Order").contains("<PropertyRef Name=\"IsActiveEntity\"/>"));
    for action in xml
        .split("<Action ")
        .skip(1)
        .map(|part| part.split("</Action>").next().unwrap())
    {
        if action.contains("Type=\"Service.Catalog\"") {
            assert!(!action.contains("Name=\"draft"), "{action}");
        }
    }
    let catalog = json_request(
        &state,
        Method::POST,
        "/CatalogSet",
        json!({"name":"A"}),
        StatusCode::CREATED,
    )
    .await;
    assert_no_drafts(&catalog);
    let id = catalog["ID"].as_str().unwrap();
    let entry = json_request(
        &state,
        Method::POST,
        &format!("/CatalogSet({id})/_Entries"),
        json!({"name":"B"}),
        StatusCode::CREATED,
    )
    .await;
    assert_no_drafts(&entry);
    let entry_id = entry["ID"].as_str().unwrap();
    json_request(
        &state,
        Method::PATCH,
        &format!("/EntrySet({entry_id})"),
        json!({"name":"Updated"}),
        StatusCode::NO_CONTENT,
    )
    .await;
    let entries = json_request(
        &state,
        Method::GET,
        "/CatalogSet?$select=name&$expand=_Entries",
        Value::Null,
        StatusCode::OK,
    )
    .await;
    assert_eq!(entries["value"][0]["_Entries"][0]["name"], "Updated");
    assert_no_drafts(&entries);
    let nested = json_request(
        &state,
        Method::GET,
        &format!("/CatalogSet({id})/_Entries({entry_id})"),
        Value::Null,
        StatusCode::OK,
    )
    .await;
    assert_no_drafts(&nested);
    let order = json_request(
        &state,
        Method::POST,
        "/OrderSet",
        json!({"name":"Draft","catalog_ID":id}),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(order["IsActiveEntity"], false);
    let order_id = order["ID"].as_str().unwrap();
    let orders = json_request(
        &state,
        Method::GET,
        "/OrderSet?$expand=catalog",
        Value::Null,
        StatusCode::OK,
    )
    .await;
    assert_no_drafts(&orders["value"][0]["catalog"]);
    assert_eq!(orders["value"][0]["catalog"]["ID"], id);
    let activated = json_request(
        &state,
        Method::POST,
        &format!("/OrderSet(ID={order_id},IsActiveEntity=false)/Service.draftActivate"),
        json!({}),
        StatusCode::OK,
    )
    .await;
    assert_eq!(activated["IsActiveEntity"], true);
    let stored = state
        .store
        .read_entity(
            "EntrySet",
            &EntityKey::new_active(entry_id),
            &Default::default(),
        )
        .await
        .unwrap();
    assert!(stored.is_active_entity());
    assert!(stored.draft_admin_data().is_none());
    let key = EntityKey::new_active(id);
    assert!(
        state
            .store
            .read_entity("CatalogSet", &key.sibling(), &Default::default())
            .await
            .is_err()
    );
    assert!(
        state
            .store
            .patch_entity("CatalogSet", &key.sibling(), &json!({"name":"Wrong"}))
            .await
            .is_err()
    );
    assert!(
        state
            .store
            .delete_entity("CatalogSet", &key.sibling())
            .await
            .is_err()
    );
    assert!(
        state
            .store
            .create_entity("CatalogSet", &json!({"IsActiveEntity":false}))
            .await
            .is_err()
    );
    assert_eq!(
        state.store.count("CatalogSet", &Default::default()).await,
        1
    );
    assert!(state.store.draft_edit("CatalogSet", &key).await.is_err());
    assert!(
        state
            .store
            .draft_activate("CatalogSet", &key.sibling())
            .await
            .is_err()
    );
    assert!(state.store.draft_prepare("CatalogSet", &key).await.is_err());
    assert!(
        state
            .store
            .read_sibling_entity("CatalogSet", &key)
            .await
            .is_err()
    );
    json_request(
        &state,
        Method::POST,
        "/CatalogSet",
        json!({"IsActiveEntity":false}),
        StatusCode::BAD_REQUEST,
    )
    .await;
    json_request(
        &state,
        Method::GET,
        &format!("/CatalogSet({id})/SiblingEntity"),
        Value::Null,
        StatusCode::NOT_FOUND,
    )
    .await;
    for uri in [
        format!("/CatalogSet(ID={id},ID={id})"),
        format!("/CatalogSet(Code={id})"),
        format!("/OrderSet({order_id})"),
        format!("/OrderSet(ID={order_id},IsActiveEntity='invalid')"),
    ] {
        json_request(
            &state,
            Method::GET,
            &uri,
            Value::Null,
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
    json_request(
        &state,
        Method::DELETE,
        &format!("/CatalogSet({id})"),
        Value::Null,
        StatusCode::NO_CONTENT,
    )
    .await;
    assert_eq!(state.store.count("EntrySet", &Default::default()).await, 0);
}

#[test]
fn mismatched_composition_modes_are_rejected() {
    let mut ddic = runtime_dictionary();
    let mut value = serde_json::to_value(&ddic).unwrap();
    value["entities"][1]["draft_enabled"] = json!(true);
    ddic = serde_json::from_value(value).unwrap();
    ddic.activate();
    let error = spec_schema::schema_from_ddic(&ddic).unwrap_err();
    assert!(
        error.to_string().contains("denselben Draft-Modus"),
        "{error}"
    );
}

#[test]
fn draft_setting_defaults_to_compatible_behavior_and_round_trips() {
    let mut value = serde_json::to_value(EntitySpec::new("Thing", "test", "Things")).unwrap();
    assert!(value.get("draft_enabled").is_none());
    let entity: EntitySpec = serde_json::from_value(value.clone()).unwrap();
    assert!(entity.draft_enabled);
    assert_eq!(entity.key_names(), &["ID", "IsActiveEntity"]);
    value["draft_enabled"] = json!(false);
    let entity: EntitySpec = serde_json::from_value(value).unwrap();
    assert_eq!(entity.key_names(), &["ID"]);
    assert_eq!(
        serde_json::to_value(entity).unwrap()["draft_enabled"],
        false
    );
}
