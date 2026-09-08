use std::path::Path;

use odata::query::Expr;
use odata::query::ODataQuery;
use odata::query::SortDirection;
use odata_yaml_store::datastore::Navigation;
use odata_yaml_store::DataStore;
use odata_yaml_store::DraftDataStore;
use odata_yaml_store::EntityKey;
use odata_yaml_store::YamlStore;
use serde_json::json;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;
fn to_uuid(n: usize) -> Uuid {
    Uuid::parse_str(&format!("00000000-0000-0000-0000-{:012X}", n)).expect("UUID mismatch")
}
fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer().with_ansi(false).with_line_number(true))
        .with(EnvFilter::from_default_env())
        .init();
    let data_dir = Path::new("testdata");
    println!("Datenverzeichnis: {}", data_dir.display());

    let store = YamlStore::open(&data_dir)?;
    let navigations = vec![
        Navigation {
            single: false,
            owning: true,
            from_set: Some("Orders".into()),
            from_field: Some("_Items".into()),
            to_set: Some("OrderItems".into()),
            to_field: Some("OrderID".into()),
        },
        Navigation {
            single: true,
            owning: false,
            from_set: Some("Orders".into()),
            from_field: Some("Customer".into()),
            to_set: Some("Customers".into()),
            to_field: Some("ID".into()),
        },
    ];
    store.set_navigations(navigations);

    let cust_mueller = to_uuid(100);
    let cust_schmidt = to_uuid(101);

    store.create_entity(
        "Customers",
        &json!({"ID": cust_mueller, "Name": "Müller GmbH"}),
    )?;
    store.draft_activate("Customers", &EntityKey::new(cust_mueller, false))?;

    store.create_entity(
        "Customers",
        &json!({"ID": cust_schmidt, "Name": "Schmidt AG"}),
    )?;
    store.draft_activate("Customers", &EntityKey::new(cust_schmidt, false))?;

    store.create_entity(
        "Orders",
        &json!({ "ID": to_uuid(1), "Customer_ID": cust_mueller, "Amount": 150, "Status": "New" }),
    )?;
    store.create_entity(
        "Orders",
        &json!({ "ID": to_uuid(2), "Customer_ID": cust_schmidt, "Amount": 480, "Status": "Released" }),
    )?;
    store.create_entity(
        "OrderItems",
        &json!({ "ID": to_uuid(10), "OrderID": to_uuid(1), "Material": "M-100", "Qty": 5 }),
    )?;
    store.create_entity(
        "OrderItems",
        &json!({ "ID": to_uuid(11), "OrderID": to_uuid(1), "Material": "M-200", "Qty": 2 }),
    )?;
    let _ = store.create_entity(
        "OrderItems",
        &json!({ "ID": to_uuid(12), "OrderID": to_uuid(2), "Material": "M-200", "Qty": 2 }),
    );

    // ------------------------------------------------------------------------------------------------
    // Tests on Data
    let key = EntityKey::new(to_uuid(1), false);

    let order = store.read_entity(
        "Orders",
        &key,
        &ODataQuery::new().expand("_Items").expand("Customer"),
    )?;
    println!(
        "\nGelesene Order: {}",
        serde_json::to_string_pretty(&order)?
    );

    store.patch_entity("Orders", &key, &json!({ "Status": "Released" }))?;
    store.draft_activate("Orders", &EntityKey::new(to_uuid(2), false))?;
    store.draft_activate("Orders", &key)?;

    let query = ODataQuery::new()
        .filter(Expr::equals_value(&true.into(), "IsActiveEntity"))
        .order_by("Amount", SortDirection::Desc)
        .expand("_Items");
    let released = store.get_collection("Orders", &query)?;
    println!(
        "\nReleased Orders (sortiert nach Amount desc): {}",
        serde_json::to_string_pretty(&released)?
    );

    // let draft = store.draft_edit("Orders", &key)?;
    // println!(
    //     "\nDraft angelegt: {}",
    //     serde_json::to_string_pretty(&draft)?
    // );

    // let draft_key = key.sibling();
    // store.patch_entity("Orders", &draft_key, &json!({ "Status": "InProgress" }))?;

    // let activated = store.draft_activate("Orders", &draft_key)?;
    // println!(
    //     "\nNach Activate: {}",
    //     serde_json::to_string_pretty(&activated)?
    // );

    // let items = store.get_collection("OrderItems", &ODataQuery::default());
    // println!("items: {:#?}", items);

    // let d = store.draft_prepare("Order", &key);
    // println!("{d:?}");

    Ok(())
}
