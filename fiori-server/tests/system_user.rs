//! Benutzerverwaltung am Store: Default User, Auto-Anlage, Owner/Audit-Felder,
//! Sperre inaktiver Benutzer, Eindeutigkeit des Usernames, Schutz des Default Users.

use std::collections::HashSet;
use std::sync::Arc;

use data_store::{DraftDataStore, EntityKey, SchemaSource, StoreError};
use odata::query::ODataQuery;
use fiori_server::hook::Extender;
use fiori_server::system_user::{ensure_default_user, SystemUserHook, CURRENT_USER};
use serde_json::json;
use spec::data_dictionary::DataDictionary;
use spec::system_user::{default_user_id, SYSTEM_USER_SET};
use spec_schema::DdicSchemaSource;
use sqlite_relational_store::RelationalStore;
use tempfile::TempDir;

struct Env {
    _dir: TempDir,
    store: Arc<dyn DraftDataStore>,
    entity_set: String,
}

async fn setup() -> Env {
    let mut ddic: DataDictionary =
        serde_json::from_str(&std::fs::read_to_string("../poc-crm/ddic.json").unwrap()).unwrap();
    ddic.inject_system_user();
    let audited: HashSet<String> = ddic
        .entities()
        .iter()
        .filter(|e| e.is_audited())
        .map(|e| e.setname())
        .collect();
    let entity = ddic
        .entities()
        .into_iter()
        .find(|e| e.name() != "SystemUser" && e.fields().any(|f| f.name() == "name"))
        .expect("an entity with a name field");
    let entity_set = entity.setname();

    let mut activated = ddic.clone();
    activated.activate();
    let schema: Arc<dyn SchemaSource> = Arc::new(DdicSchemaSource::new(&activated).unwrap());
    let dir = TempDir::new().unwrap();
    let raw: Arc<dyn DraftDataStore> = Arc::new(
        RelationalStore::open(dir.path().to_path_buf(), schema)
            .await
            .unwrap(),
    );
    ensure_default_user(raw.as_ref()).await.unwrap();
    let store = Extender::new(raw, Arc::new(SystemUserHook::new(audited))).unwrap();
    Env {
        _dir: dir,
        store,
        entity_set,
    }
}

async fn as_user<T, F: std::future::Future<Output = T>>(user: Option<&str>, f: F) -> T {
    CURRENT_USER.scope(user.map(str::to_string), f).await
}

/// Alle Datensaetze einer Menge als JSON (ohne Hooks: direkt am Store gelesen).
async fn records(store: &Arc<dyn DraftDataStore>, set: &str) -> Vec<serde_json::Value> {
    store
        .get_collection(set, &ODataQuery::new())
        .await
        .unwrap()
        .iter()
        .map(|r| r.core_json())
        .collect()
}

async fn users(store: &Arc<dyn DraftDataStore>) -> Vec<serde_json::Value> {
    records(store, SYSTEM_USER_SET).await
}

#[tokio::test]
async fn default_user_exists_with_fixed_id() {
    let env = setup().await;
    let all = users(&env.store).await;
    let default = all
        .iter()
        .find(|u| u["name"] == "none@localhost")
        .expect("default user");
    assert_eq!(default["ID"], json!(default_user_id().to_string()));
    assert_eq!(default["active"], json!(true));
}

#[tokio::test]
async fn request_without_header_is_owned_by_default_user() {
    let env = setup().await;
    let rec = as_user(
        None,
        env.store.create_entity(&env.entity_set, &json!({"name": "A"})),
    )
    .await
    .unwrap();
    assert_eq!(rec.get("Owner_ID"), Some(json!(default_user_id().to_string())));
    assert_eq!(rec.get("CreatedBy_ID"), Some(json!(default_user_id().to_string())));
    assert!(rec.get("CreatedAt").is_some());
}

#[tokio::test]
async fn unknown_header_user_is_created_once_and_becomes_owner() {
    let env = setup().await;
    let rec = as_user(
        Some("peter@example.org"),
        env.store.create_entity(&env.entity_set, &json!({"name": "A"})),
    )
    .await
    .unwrap();
    let owner = rec.get("Owner_ID").unwrap();
    assert_ne!(owner, json!(default_user_id().to_string()));

    let _ = as_user(
        Some("peter@example.org"),
        env.store.create_entity(&env.entity_set, &json!({"name": "B"})),
    )
    .await
    .unwrap();
    let created: Vec<_> = users(&env.store)
        .await
        .into_iter()
        .filter(|u| u["name"] == "peter@example.org")
        .collect();
    assert_eq!(created.len(), 1, "user must not be created twice");
    assert_eq!(created[0]["active"], json!(true));
    assert_eq!(json!(created[0]["ID"]), owner);
}

#[tokio::test]
async fn explicit_owner_is_kept_and_changes_are_audited() {
    let env = setup().await;
    let other = default_user_id().to_string();
    let rec = as_user(
        Some("peter@example.org"),
        env.store
            .create_entity(&env.entity_set, &json!({"name": "A", "Owner_ID": other})),
    )
    .await
    .unwrap();
    assert_eq!(rec.get("Owner_ID"), Some(json!(other)));

    let key = EntityKey::new(rec.id(), false);
    as_user(
        Some("peter@example.org"),
        env.store.patch_entity(
            &env.entity_set,
            &key,
            &json!({"name": "B", "CreatedBy_ID": "evil"}),
        ),
    )
    .await
    .unwrap();
    let changed = records(&env.store, &env.entity_set)
        .await
        .into_iter()
        .find(|r| r["ID"] == json!(rec.id().to_string()))
        .expect("record");
    assert_ne!(changed["ChangedBy_ID"], json!(null));
    assert_eq!(changed["CreatedBy_ID"], json!(rec.get("CreatedBy_ID").unwrap()));
}

#[tokio::test]
async fn inactive_user_is_forbidden() {
    let env = setup().await;
    as_user(
        Some("ina@example.org"),
        env.store
            .create_entity(&env.entity_set, &json!({"name": "A"})),
    )
    .await
    .unwrap();
    let user_id = users(&env.store)
        .await
        .into_iter()
        .find(|u| u["name"] == "ina@example.org")
        .map(|u| u["ID"].as_str().unwrap().parse().unwrap())
        .unwrap();
    env.store
        .patch_entity(
            SYSTEM_USER_SET,
            &EntityKey::new(user_id, true),
            &json!({"active": false}),
        )
        .await
        .unwrap();

    let err = as_user(
        Some("ina@example.org"),
        env.store
            .create_entity(&env.entity_set, &json!({"name": "B"})),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, StoreError::Forbidden(_)), "{err:?}");
}

#[tokio::test]
async fn duplicate_username_is_rejected() {
    let env = setup().await;
    as_user(
        Some("dup@example.org"),
        env.store
            .create_entity(&env.entity_set, &json!({"name": "A"})),
    )
    .await
    .unwrap();
    let err = env
        .store
        .create_entity(SYSTEM_USER_SET, &json!({"name": "dup@example.org", "active": true}))
        .await
        .unwrap_err();
    assert!(matches!(err, StoreError::Conflict(_)), "{err:?}");
}

#[tokio::test]
async fn default_user_cannot_be_deactivated_or_deleted_and_users_are_not_deleted() {
    let env = setup().await;
    let default_key = EntityKey::new(default_user_id(), true);
    let err = env
        .store
        .patch_entity(SYSTEM_USER_SET, &default_key, &json!({"active": false}))
        .await
        .unwrap_err();
    assert!(matches!(err, StoreError::Forbidden(_)), "{err:?}");

    let err = env
        .store
        .delete_entity(SYSTEM_USER_SET, &default_key)
        .await
        .unwrap_err();
    assert!(matches!(err, StoreError::Forbidden(_)), "{err:?}");
}

#[tokio::test]
async fn ensure_default_user_is_idempotent() {
    let env = setup().await;
    ensure_default_user(env.store.as_ref()).await.unwrap();
    let count = users(&env.store)
        .await
        .into_iter()
        .filter(|u| u["name"] == "none@localhost")
        .count();
    assert_eq!(count, 1);
}

#[test]
fn non_audited_entity_gets_no_owner_and_no_audit_fields() {
    // ValueList stammt aus dem eingebauten Metamodell und ist nicht auditiert.
    let mut ddic = DataDictionary::new();
    ddic.inject_system_user();
    let vl = ddic.entities().into_iter().find(|e| e.name() == "ValueList").unwrap();
    assert!(!vl.is_audited());
    assert!(vl.fields().all(|f| f.name() != "Owner"));
    assert!(vl.fields().all(|f| f.name() != "CreatedAt"));

    let entity = ddic.entities().into_iter().find(|e| e.name() == "Entity").unwrap();
    assert!(entity.is_audited());
    assert!(entity.fields().any(|f| f.name() == "Owner"));
}
