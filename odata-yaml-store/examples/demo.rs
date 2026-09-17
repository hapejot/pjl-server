use std::path::Path;

use odata::query::Expr;
use odata::query::ODataQuery;
use odata::query::SortDirection;
use odata_yaml_store::{Navigation, DataStore, DraftDataStore, EntityKey, YamlStore};
use serde_json::json;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;
fn to_uuid(n: usize) -> Uuid {
    Uuid::parse_str(&format!("00000000-0000-0000-0000-{:012X}", n)).expect("UUID mismatch")
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
    ).await?;
    store.draft_activate("Customers", &EntityKey::new(cust_mueller, false)).await?;

    store.create_entity(
        "Customers",
        &json!({"ID": cust_schmidt, "Name": "Schmidt AG"}),
    ).await?;
    store.draft_activate("Customers", &EntityKey::new(cust_schmidt, false)).await?;

    store.create_entity(
        "Orders",
        &json!({ "ID": to_uuid(1), "Customer_ID": cust_mueller, "Amount": 150, "Status": "New" }),
    ).await?;
    store.create_entity(
        "Orders",
        &json!({ "ID": to_uuid(2), "Customer_ID": cust_schmidt, "Amount": 480, "Status": "Released" }),
    ).await?;
    store.create_entity(
        "OrderItems",
        &json!({ "ID": to_uuid(10), "OrderID": to_uuid(1), "Material": "M-100", "Qty": 5 }),
    ).await?;
    store.create_entity(
        "OrderItems",
        &json!({ "ID": to_uuid(11), "OrderID": to_uuid(1), "Material": "M-200", "Qty": 2 }),
    ).await?;
    let _ = store.create_entity(
        "OrderItems",
        &json!({ "ID": to_uuid(12), "OrderID": to_uuid(2), "Material": "M-200", "Qty": 2 }),
    ).await;

    // ------------------------------------------------------------------------------------------------
    // Tests on Data
    let key = EntityKey::new(to_uuid(1), false);

    let order = store.read_entity(
        "Orders",
        &key,
        &ODataQuery::new().expand("_Items").expand("Customer"),
    ).await?;
    println!(
        "\nGelesene Order: {}",
        serde_json::to_string_pretty(&order)?
    );

    store.patch_entity("Orders", &key, &json!({ "Status": "Released" })).await?;
    store.draft_activate("Orders", &EntityKey::new(to_uuid(2), false)).await?;
    store.draft_activate("Orders", &key).await?;

    let query = ODataQuery::new()
        .filter(Expr::equals_value(&true.into(), "IsActiveEntity"))
        .order_by("Amount", SortDirection::Desc)
        .expand("_Items");
    let released = store.get_collection("Orders", &query).await?;
    println!(
        "\nReleased Orders (sortiert nach Amount desc): {}",
        serde_json::to_string_pretty(&released)?
    );

    // let draft = store.draft_edit("Orders", &key).await?;
    // println!(
    //     "\nDraft angelegt: {}",
    //     serde_json::to_string_pretty(&draft)?
    // );

    // let draft_key = key.sibling();
    // store.patch_entity("Orders", &draft_key, &json!({ "Status": "InProgress" })).await?;

    // let activated = store.draft_activate("Orders", &draft_key).await?;
    // println!(
    //     "\nNach Activate: {}",
    //     serde_json::to_string_pretty(&activated)?
    // );

    // let items = store.get_collection("OrderItems", &ODataQuery::default()).await;
    // println!("items: {:#?}", items);

    // let d = store.draft_prepare("Order", &key).await;
    // println!("{d:?}");

    Ok(())
}
