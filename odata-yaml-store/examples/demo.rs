use odata_yaml_store::{
    DataStore, DraftActionExt, EntityKey, EntityMetadata, FilterExpr, NavigationDef, ODataQuery,
    OrderByItem, SortDirection, YamlStore,
};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Datenverzeichnis im aktuellen Arbeitsverzeichnis (wird bei Bedarf angelegt)
    let data_dir = std::env::temp_dir().join("odata_yaml_store_demo");
    println!("Datenverzeichnis: {}", data_dir.display());

    // 1. Metadaten registrieren: Key-Felder + Navigationen
    let mut metadata = EntityMetadata::new();
    metadata.register_key("Orders", &["ID"]);
    metadata.register_key("OrderItems", &["ID"]);
    metadata.register_navigation(
        "Orders",
        "Items",
        NavigationDef {
            target_set: "OrderItems".to_string(),
            foreign_key_field: "OrderID".to_string(),
        },
    );

    let store = YamlStore::open(&data_dir, metadata)?;

    // 2. Entities anlegen
    store.create_entity(
        "Orders",
        &json!({ "ID": "1", "CustomerName": "Mueller GmbH", "Amount": 150, "Status": "New" }),
        None,
    )?;
    store.create_entity(
        "Orders",
        &json!({ "ID": "2", "CustomerName": "Schmidt AG", "Amount": 480, "Status": "Released" }),
        None,
    )?;
    store.create_entity(
        "OrderItems",
        &json!({ "ID": "10", "OrderID": "1", "Material": "M-100", "Qty": 5 }),
        None,
    )?;
    store.create_entity(
        "OrderItems",
        &json!({ "ID": "11", "OrderID": "1", "Material": "M-200", "Qty": 2 }),
        None,
    )?;

    // 3. Einzelne Entity lesen
    let key = EntityKey::new([("ID".to_string(), "1".to_string())], true);
    let order = store.read_entity("Orders", &key, &ODataQuery::default())?;
    println!("\nGelesene Order: {}", serde_json::to_string_pretty(&order)?);

    // 4. Patch
    store.patch_entity("Orders", &key, &json!({ "Status": "Released" }))?;

    // 5. Collection mit $filter + $orderby
    let query = ODataQuery {
        filter: Some(FilterExpr::eq("Status", "Released")),
        orderby: vec![OrderByItem {
            property: "Amount".to_string(),
            direction: SortDirection::Desc,
        }],
        ..Default::default()
    };
    let released = store.get_collection("Orders", &query, None)?;
    println!(
        "\nReleased Orders (sortiert nach Amount desc): {}",
        serde_json::to_string_pretty(&released)?
    );

    // 6. Draft-Workflow: Edit -> Patch -> Activate
    let draft = store.draft_edit("Orders", &key)?;
    println!("\nDraft angelegt: {}", serde_json::to_string_pretty(&draft)?);

    let draft_key = key.sibling();
    store.patch_entity("Orders", &draft_key, &json!({ "Status": "InProgress" }))?;

    let activated = store.draft_activate("Orders", &draft_key)?;
    println!(
        "\nNach Activate: {}",
        serde_json::to_string_pretty(&activated)?
    );

    // 7. Aufräumen (Demo-Verzeichnis wieder löschen)
    std::fs::remove_dir_all(&data_dir).ok();

    Ok(())
}
