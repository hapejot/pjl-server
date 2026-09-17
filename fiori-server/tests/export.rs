//! Der Uebergabepunkt zwischen den beiden Servern: was der Designer
//! exportiert, muss der Laufzeitserver ohne Nacharbeit laden koennen.

use std::sync::Arc;

use data_store::schema::Schema;
use fiori_server::AppState;
use fiori_server::config::{ServerConfig, ServerMode};
use fiori_server::export::export_dictionary;
use odata_yaml_store::{DataStore, YamlStore};
use serde_json::json;
use spec::data_dictionary::DataDictionary;
use tempfile::TempDir;

const PACKAGE_ID: &str = "00000000-0000-0000-0000-0000000000a1";
const ORDER_ID: &str = "00000000-0000-0000-0000-000000000001";
const ITEM_ID: &str = "00000000-0000-0000-0000-000000000002";

/// Ein Designer, der ein kleines Modell (Order 1:n OrderItem) im Store hat.
fn designer_with_model() -> (TempDir, TempDir, AppState) {
    let data = TempDir::new().unwrap();
    let export = TempDir::new().unwrap();
    let store = Arc::new(YamlStore::open(data.path()).unwrap());

    let create = |set: &str, body: serde_json::Value| {
        store.create_entity(set, &body).unwrap();
    };
    create(
        "PackageSet",
        json!({ "ID": PACKAGE_ID, "IsActiveEntity": true, "name": "sales" }),
    );
    create(
        "EntitySet",
        json!({ "ID": ORDER_ID, "IsActiveEntity": true, "name": "Order",
                "plural": "Orders", "package_ID": PACKAGE_ID }),
    );
    create(
        "EntitySet",
        json!({ "ID": ITEM_ID, "IsActiveEntity": true, "name": "OrderItem",
                "plural": "OrderItems", "package_ID": PACKAGE_ID }),
    );
    create(
        "FieldSet",
        json!({ "IsActiveEntity": true, "entity_ID": ORDER_ID, "name": "amount",
                "label": "Amount", "type": "Edm.Decimal", "precision": 15, "scale": 2 }),
    );
    create(
        "RelationSet",
        json!({ "IsActiveEntity": true, "name": "order_items", "owned": true,
                "one_ID": ORDER_ID, "source_field": "_Items",
                "many_ID": ITEM_ID, "target_field": "order" }),
    );
    create(
        "AppSet",
        json!({ "IsActiveEntity": true, "name": "orders", "title": "Orders",
                "main_entity_ID": ORDER_ID, "package_ID": PACKAGE_ID }),
    );

    let mut config = ServerConfig::defaults(ServerMode::Designer);
    config.export_dir = export.path().to_path_buf();
    let state = AppState::new(store, Arc::new(config), DataDictionary::new());
    state.reload();
    (data, export, state)
}

#[test]
fn export_writes_a_dictionary_the_runtime_can_load() {
    let (_data, export, state) = designer_with_model();

    let report = export_dictionary(&state).expect("export");
    assert_eq!(report.entities, 2);
    assert_eq!(report.apps, 1);
    assert_eq!(report.relationships, 1);

    // Genau der Weg, den `server::load_dictionary` im Laufzeitserver geht.
    let raw = std::fs::read(export.path().join("ddic.json")).expect("ddic.json");
    let mut loaded: DataDictionary = serde_json::from_slice(&raw).expect("parse ddic");
    loaded.activate();

    let names: Vec<&str> = loaded.entity_names().collect();
    assert!(names.contains(&"Order"), "got {names:?}");
    assert!(names.contains(&"OrderItem"), "got {names:?}");
    assert_eq!(loaded.apps().count(), 1);

    // Das mitgelieferte Schema muss dem entsprechen, was die Laufzeit aus dem
    // Dictionary selbst ableiten wuerde - sonst haengt das Verhalten davon ab,
    // ob `--schema` gesetzt ist.
    let written = Schema::load(export.path().join("schema.json")).expect("schema.json");
    let derived = spec_schema::schema_from_ddic(&loaded).expect("derive");
    let mut written_sets: Vec<&str> = written.sets().map(|s| s.name.as_str()).collect();
    let mut derived_sets: Vec<&str> = derived.sets().map(|s| s.name.as_str()).collect();
    written_sets.sort();
    derived_sets.sort();
    assert_eq!(written_sets, derived_sets);
    assert_eq!(written_sets, vec!["OrderItemSet", "OrderSet"]);
}

#[test]
fn the_metamodel_stays_in_the_designer() {
    let (_data, export, state) = designer_with_model();
    export_dictionary(&state).expect("export");

    let raw = std::fs::read(export.path().join("ddic.json")).unwrap();
    let loaded: DataDictionary = serde_json::from_slice(&raw).unwrap();

    // Sonst wuerde der Laufzeitserver die Pflegeoberflaeche des Designers
    // noch einmal anbieten - inklusive Tabellen fuer das Metamodell.
    for meta in ["Entity", "Field", "Relation", "Package", "App"] {
        assert!(
            !loaded.entity_names().any(|n| n == meta),
            "metamodel entity '{meta}' leaked into the export"
        );
    }
    assert!(!loaded.apps().any(|a| a.name() == "entities"));
}

#[test]
fn compositions_survive_the_round_trip() {
    let (_data, export, state) = designer_with_model();
    export_dictionary(&state).expect("export");

    let schema = Schema::load(export.path().join("schema.json")).unwrap();
    let order = schema.set("OrderSet").expect("OrderSet");
    let items = order
        .associations()
        .find(|a| a.target_set == "OrderItemSet")
        .expect("association to OrderItemSet");
    // `owned` geht beim Aktivieren verloren und wird in `spec-schema` aus den
    // Relationships zurueckgewonnen; ohne das faende die Laufzeit keine
    // Draft-Kinder und kein Cascade-Delete.
    assert_eq!(items.ownership, data_store::schema::Ownership::Composition);
}
