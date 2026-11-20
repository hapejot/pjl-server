use std::io::{Cursor, Write};

use data_store::{DataStore, EntityKey};
use fiori_server::{ddic_file, ddic_packages, ddic_store::DdicStore};
use odata::query::ODataQuery;
use serde_json::{Value, json};
use tempfile::TempDir;
use tracing::{info, subscriber::DefaultGuard};

fn write(path: &std::path::Path, value: &Value) {
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

fn test_subscriber() -> impl tracing::Subscriber {
    tracing_subscriber::fmt()
        .with_test_writer() // integriert sich mit `cargo test`'s Output-Capturing
        .with_max_level(tracing::Level::TRACE)
        .finish()
}

fn fixture() -> (DefaultGuard, TempDir) {
    let guard = tracing::subscriber::set_default(test_subscriber());
    let dir = TempDir::new().unwrap();
    std::fs::create_dir(dir.path().join("packages")).unwrap();
    write(
        &dir.path().join("ddic.json"),
        &json!({
            "packages": [{"name": "one"}, {"name": "two"}],
            "package_files": ["one", "two"],
            "entities": [], "relationships": [],
            "apps": [{"name":"orders","title":"Orders","main_entity":"Order","package":{"name":"one"}}]
        }),
    );
    write(
        &dir.path().join("packages/one.json"),
        &json!({
            "entities": [{"name":"Order","package":"one","type_name_plural":"Orders","fields":[
                {"Atom":{"name":"name","label":"Name","edm_type":"Edm.String"}}
            ]}],
            "value_lists": [{"id":"79714635-4b3e-51a6-be78-dc009beb5f3c","name":"States","values":[]}],
            "components": [{"name":"TimeRange","kind":"time_range"}],
            "extra": {"unchanged": true}
        }),
    );
    write(&dir.path().join("packages/two.json"), &json!({}));
    (guard, dir)
}

fn read(path: &std::path::Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn merges_packages_and_rejects_duplicate_definitions() {
    let (_guard, dir) = fixture();
    let ddic = ddic_file::load(&dir.path().join("ddic.json")).unwrap();
    assert!(ddic.entities().iter().any(|e| e.name() == "Order"));
    assert_eq!(ddic.value_lists().len(), 1);
    write(
        &dir.path().join("packages/two.json"),
        &json!({
            "entities": [{"name":"Order","package":"two","type_name_plural":"Orders","fields":[]}]
        }),
    );
    let err = ddic_file::load(&dir.path().join("ddic.json")).unwrap_err();
    assert!(err.to_string().contains("mehrfach definiert"), "{err:#}");
}

#[tokio::test]
async fn designer_moves_value_lists_and_components_and_rejects_unknown_package() {
    let (_guard, dir) = fixture();
    info!("designer move");
    let store = DdicStore::open(dir.path().join("ddic.json")).unwrap();
    for set in ["ValueListSet", "ComponentSet"] {
        let record = store
            .get_collection(set, &ODataQuery::new())
            .await
            .unwrap()
            .remove(0);
        assert_eq!(record.get("package"), Some(json!("one")));
        let key = EntityKey::new_active(record.id().to_string());
        assert!(
            store
                .patch_entity(set, &key, &json!({"package":"missing"}))
                .await
                .is_err()
        );
        store
            .patch_entity(set, &key, &json!({"package":"two"}))
            .await
            .unwrap();
    }
    let one = read(&dir.path().join("packages/one.json"));
    let two = read(&dir.path().join("packages/two.json"));
    assert!(one.get("components").is_none());
    assert!(one.get("value_lists").is_none());
    assert_eq!(two["components"][0]["package"], "two");
    assert_eq!(two["value_lists"][0]["package"], "two");
}

#[tokio::test]
async fn designer_moves_entity_between_files_and_preserves_packages() {
    let (_guard, dir) = fixture();
    let store = DdicStore::open(dir.path().join("ddic.json")).unwrap();
    let entities = store
        .get_collection("EntitySet", &ODataQuery::new())
        .await
        .unwrap();
    let packages = store
        .get_collection("PackageSet", &ODataQuery::new())
        .await
        .unwrap();
    let order = entities
        .iter()
        .find(|r| r.get("name") == Some(json!("Order")))
        .unwrap();
    let two = packages
        .iter()
        .find(|r| r.get("name") == Some(json!("two")))
        .unwrap();
    store
        .patch_entity(
            "EntitySet",
            &EntityKey::new_active(order.id().to_string()),
            &json!({"package_ID": two.id().to_string()}),
        )
        .await
        .unwrap();
    let one = read(&dir.path().join("packages/one.json"));
    let two = read(&dir.path().join("packages/two.json"));
    let root = read(&dir.path().join("ddic.json"));
    assert!(one.get("entities").is_none());
    assert_eq!(one["value_lists"][0]["name"], "States");
    assert_eq!(one["extra"]["unchanged"], true);
    assert_eq!(two["entities"][0]["name"], "Order");
    assert_eq!(two["entities"][0]["package"], "two");
    assert_eq!(root["apps"][0]["name"], "orders");
    let reopened = DdicStore::open(dir.path().join("ddic.json")).unwrap();
    assert_eq!(
        reopened
            .get_collection("EntitySet", &ODataQuery::new())
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn designer_saves_vorfertigung_with_measure_columns() {
    let _guard = tracing::subscriber::set_default(test_subscriber());
    info!("designer save vorfertigung");
    let dir = TempDir::new().unwrap();
    std::fs::create_dir(dir.path().join("packages")).unwrap();
    std::fs::write(
        dir.path().join("ddic.json"),
        include_str!("../../vorfertigung/ddic.json"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("packages/units.json"),
        include_str!("../../vorfertigung/packages/units.json"),
    )
    .unwrap();
    let store = DdicStore::open(dir.path().join("ddic.json")).unwrap();
    let entity = store
        .get_collection("EntitySet", &ODataQuery::new())
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.get("name") == Some(json!("UnitOfMeasure")))
        .unwrap();
    store
        .patch_entity(
            "EntitySet",
            &EntityKey::new_active(entity.id().to_string()),
            &json!({"type_name":"Testeinheit"}),
        )
        .await
        .unwrap();
    assert_eq!(
        read(&dir.path().join("packages/units.json"))["entities"][0]["type_name"],
        "Testeinheit"
    );
}

#[tokio::test]
async fn external_package_edit_is_not_overwritten() {
    let (_guard, dir) = fixture();
    let store = DdicStore::open(dir.path().join("ddic.json")).unwrap();
    let entity = store
        .get_collection("EntitySet", &ODataQuery::new())
        .await
        .unwrap()
        .remove(0);
    write(
        &dir.path().join("packages/two.json"),
        &json!({"extra":"external"}),
    );
    let err = store
        .patch_entity(
            "EntitySet",
            &EntityKey::new_active(entity.id().to_string()),
            &json!({"type_name":"Changed"}),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("extern geaendert"), "{err}");
    assert_eq!(
        read(&dir.path().join("packages/two.json"))["extra"],
        "external"
    );
}

#[test]
fn zip_source_loads_packages_relative_to_dictionary() {
    let (_guard, dir) = fixture();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, path) in [
        ("model/ddic.json", "ddic.json"),
        ("model/packages/one.json", "packages/one.json"),
        ("model/packages/two.json", "packages/two.json"),
    ] {
        zip.start_file(name, options).unwrap();
        zip.write_all(&std::fs::read(dir.path().join(path)).unwrap())
            .unwrap();
    }
    let data = zip.finish().unwrap().into_inner();
    let repo =
        file_repo::ZipRepository::from_bytes(data.into(), &file_repo::Limits::default()).unwrap();
    let (value, _) = ddic_packages::load(&repo, "model/ddic.json").unwrap();
    let ddic: spec::data_dictionary::DataDictionary = serde_json::from_value(value).unwrap();
    assert!(ddic.entities().iter().any(|e| e.name() == "Order"));
}
