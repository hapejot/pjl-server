use std::path::Path;

use odata::query::ODataQuery;
use odata::query::OrderByItem;
use odata::query::SortDirection;
use odata_yaml_store::DataStore;
use odata_yaml_store::DraftDataStore;
use odata_yaml_store::EntityKey;
use odata_yaml_store::YamlStore;
use serde_json::json;
use uuid::Uuid;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = Path::new("testdata");
    println!("Datenverzeichnis: {}", data_dir.display());

    let store = YamlStore::open(&data_dir)?;

    let _ = store.create_entity(
        "Orders",
        &json!({ "ID": "1", "CustomerName": "Mueller GmbH", "Amount": 150, "Status": "New" }),
    );
    let _ = store.create_entity(
        "Orders",
        &json!({ "ID": "2", "CustomerName": "Schmidt AG", "Amount": 480, "Status": "Released" }),
    );
    let _ = store.create_entity(
        "OrderItems",
        &json!({ "ID": "10", "OrderID": "1", "Material": "M-100", "Qty": 5 }),
    );
    let _ = store.create_entity(
        "OrderItems",
        &json!({ "ID": "11", "OrderID": "1", "Material": "M-200", "Qty": 2 }),
    );
    let _ = store.create_entity(
        "OrderItems",
        &json!({ "ID": "10", "OrderID": "2", "Material": "M-200", "Qty": 2 }),
    );

    let key = EntityKey::new(
        Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
        true,
    );
    let order = store.read_entity("Orders", &key, &ODataQuery::new())?;
    println!(
        "\nGelesene Order: {}",
        serde_json::to_string_pretty(&order)?
    );

    store.patch_entity("Orders", &key, &json!({ "Status": "Released" }))?;

    let query = ODataQuery {
        filter: None,
        orderby: vec![OrderByItem {
            property: "Amount".to_string(),
            direction: SortDirection::Desc,
        }],
        ..Default::default()
    };
    let released = store.get_collection("Orders", &query)?;
    println!(
        "\nReleased Orders (sortiert nach Amount desc): {}",
        serde_json::to_string_pretty(&released)?
    );

    let draft = store.draft_edit("Orders", &key)?;
    println!(
        "\nDraft angelegt: {}",
        serde_json::to_string_pretty(&draft)?
    );

    let draft_key = key.sibling();
    store.patch_entity("Orders", &draft_key, &json!({ "Status": "InProgress" }))?;

    let activated = store.draft_activate("Orders", &draft_key)?;
    println!(
        "\nNach Activate: {}",
        serde_json::to_string_pretty(&activated)?
    );

    let key = EntityKey::new(
        Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
        true,
    );
    let items = store.get_collection("OrderItems", &ODataQuery::default());
    println!("items: {:#?}", items);

    let d = store.draft_prepare("Order", &key);
    println!("{d:?}");

    Ok(())
}
