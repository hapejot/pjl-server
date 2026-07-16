use odata_yaml_store::{DataStore, EntityMetadata, YamlStore};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let entity_configs = "EntityConfigs";
    let app = "FioriApp";
    let stdkey = ["ID"];

    let mut metadata = EntityMetadata::new();

    metadata.register_key(entity_configs, &stdkey);
    metadata.register_key(app, &stdkey);

    let store = YamlStore::open("data", metadata).unwrap();
    // metadata.register_key("Orders", &["ID"]);
    // metadata.register_key("OrderItems", &["ID"]);
    // metadata.register_navigation(
    //     "Orders",
    //     "Items",
    //     NavigationDef {
    //         target_set: "OrderItems".to_string(),
    //         foreign_key_field: "OrderID".to_string(),
    //     },
    // );

    // 2. Entities anlegen
    let _ = store.create_entity(
        entity_configs,
        &json!({ "ID": "entity-1", "Name": "Entity" }),
    );
    let _ = store.create_entity(
        entity_configs,
        &json!({ "ID": "entity-2", "Name": "FioriApp" }),
    );
    let _ = store.create_entity(
        entity_configs,
        &json!({ "ID": "entity-3", "Name": "Relation" }),
    );
    let _ = store.create_entity(
        entity_configs,
        &json!({ "ID": "entity-4", "Name": "Field" } ),
    );
    let _ = store.create_entity(
        entity_configs,
        &json!({ "ID": "entity-3", "Name": "Relation" }),
    );

    let _ = store.create_entity(
        app,
        &json!({
            "ID": "app-1", "Name": "entity.app", "Title": "Entities", "Subtitle":"Maintain Entities", "Entity": "Entity", "Action":"maintain"
        }),
    );
    Ok(())
}
