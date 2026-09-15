use std::sync::Arc;

use data_store::*;
use odata::query::{Expr, ODataQuery, SortDirection};
use serde_json::json;
use sqlite_relational_store::RelationalStore;
use uuid::Uuid;

fn main() -> anyhow::Result<()> {
    let s = match Schema::load("schema.json") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            schema()
        }
    };
    s.save("schema.json")?;
    let store = RelationalStore::open("rel-data", Arc::new(StaticSchema::new(s)))?;
    let cust_mueller = to_uuid(100);
    let cust_schmidt = to_uuid(101);

    store.create_entity(
        CUSTOMERS,
        &json!({"ID": cust_mueller, "Name": "Müller GmbH"}),
    )?;
    store.draft_activate(CUSTOMERS, &EntityKey::new(cust_mueller, false))?;

    store.create_entity(
        CUSTOMERS,
        &json!({"ID": cust_schmidt, "Name": "Schmidt AG"}),
    )?;
    store.draft_activate(CUSTOMERS, &EntityKey::new(cust_schmidt, false))?;

    store.create_entity(
        ORDERS,
        &json!({ "ID": to_uuid(1), "Customer_ID": cust_mueller, "Amount": 150, "Status": "New" }),
    )?;
    store.create_entity(
        ORDERS,
        &json!({ "ID": to_uuid(2), "Customer_ID": cust_schmidt, "Amount": 480, "Status": "Released" }),
    )?;
    store.create_entity(
        ITEMS,
        &json!({ "ID": to_uuid(10), "Order_ID": to_uuid(1), "Material": "M-100", "Qty": 5 }),
    )?;
    store.create_entity(
        ITEMS,
        &json!({ "ID": to_uuid(11), "Order_ID": to_uuid(1), "Material": "M-200", "Qty": 2 }),
    )?;
    let _ = store.create_entity(
        ITEMS,
        &json!({ "ID": to_uuid(12), "Order_ID": to_uuid(2), "Material": "M-200", "Qty": 2 }),
    );

    // ------------------------------------------------------------------------------------------------
    // Tests on Data
    let key = EntityKey::new(to_uuid(1), false);

    let order = store.read_entity(
        ORDERS,
        &key,
        &ODataQuery::new().expand("_Items").expand("Customer"),
    )?;
    println!(
        "\nGelesene Order: {}",
        serde_json::to_string_pretty(&order)?
    );

    store.patch_entity(ORDERS, &key, &json!({ "Status": "Released" }))?;
    store.draft_activate(ORDERS, &EntityKey::new(to_uuid(2), false))?;
    store.draft_activate(ORDERS, &key)?;

    let query = ODataQuery::new()
        .filter(Expr::equals_value(&true.into(), "IsActiveEntity"))
        .order_by("Amount", SortDirection::Desc)
        .expand("_Items");
    let released = store.get_collection(ORDERS, &query)?;
    println!(
        "\nReleased Orders (sortiert nach Amount desc): {}",
        serde_json::to_string_pretty(&released)?
    );
    Ok(())
}

fn to_uuid(n: usize) -> Uuid {
    Uuid::parse_str(&format!("00000000-0000-0000-0000-{:012X}", n)).expect("UUID mismatch")
}
const ORDERS: &str = "OrderSet";
const CUSTOMERS: &str = "CustomerSet";
const ITEMS: &str = "OrderItemSet";
/// Das Schema, gegen das die Suite laeuft: Order 1:n Item (Komposition),
/// Order n:1 Customer (Assoziation).
pub fn schema() -> Schema {
    Schema::builder()
        .set(
            EntitySet::new(ORDERS, "Order")
                .drafts()
                .column(Column::new(
                    "Status",
                    DataType::String {
                        max_length: Some(40),
                    },
                ))
                .column(Column::new("Amount", DataType::Int32))
                .column(Column::foreign_key("Customer_ID", CUSTOMERS))
                .association(Association {
                    name: "_Items".into(),
                    target_set: ITEMS.into(),
                    cardinality: Cardinality::ToMany,
                    ownership: Ownership::Composition,
                    foreign_key: ForeignKeySide::There {
                        column: "Order_ID".into(),
                    },
                    reverse: Some("Order".into()),
                })
                .association(Association {
                    name: "Customer".into(),
                    target_set: CUSTOMERS.into(),
                    cardinality: Cardinality::ToOne,
                    ownership: Ownership::Association,
                    foreign_key: ForeignKeySide::Here {
                        column: "Customer_ID".into(),
                    },
                    reverse: None,
                }),
        )
        .set(
            EntitySet::new(ITEMS, "Item")
                .drafts()
                .column(Column::new(
                    "Material",
                    DataType::String {
                        max_length: Some(40),
                    },
                ))
                .column(Column::new("Qty", DataType::Int32))
                .column(Column::foreign_key("Order_ID", ORDERS))
                .association(Association {
                    name: "Order".into(),
                    target_set: ORDERS.into(),
                    cardinality: Cardinality::ToOne,
                    ownership: Ownership::Composition,
                    foreign_key: ForeignKeySide::Here {
                        column: "Order_ID".into(),
                    },
                    reverse: Some("_Items".into()),
                }),
        )
        .set(
            EntitySet::new(CUSTOMERS, "Customer")
                .drafts()
                .column(Column::new(
                    "Name",
                    DataType::String {
                        max_length: Some(80),
                    },
                )),
        )
        .build()
        .expect("konformes Testschema")
}
