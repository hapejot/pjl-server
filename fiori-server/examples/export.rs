use std::sync::Arc;

use data_store::DataStore;
use fiori_server::{
    config::{ServerConfig, ServerMode},
    export::export_dictionary,
    AppState,
};
use odata_yaml_store::YamlStore;
use serde_json::json;
use spec::data_dictionary::DataDictionary;

const PACKAGE_ID: &str = "00000000-0000-0000-0000-0000000000a1";
const ORDER_ID: &str = "00000000-0000-0000-0000-000000000001";
const ITEM_ID: &str = "00000000-0000-0000-0000-000000000002";
const RELATION_ID: &str = "a6950718-bfe8-4408-83a0-000714db7c36";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let store = Arc::new(YamlStore::open("sample-data")?);
    async fn create(store: &YamlStore, set: &str, body: serde_json::Value) {
        store.create_entity(set, &body).await.unwrap();
    }
    create(
        &store,
        "PackageSet",
        json!({ "ID": PACKAGE_ID, "IsActiveEntity": true, "name": "sales" }),
    )
    .await;
    create(
        &store,
        "EntitySet",
        json!({ "ID": ORDER_ID, "IsActiveEntity": true, "name": "Order",
                "plural": "Orders", "package_ID": PACKAGE_ID }),
    )
    .await;
    create(
        &store,
        "EntitySet",
        json!({ "ID": ITEM_ID, "IsActiveEntity": true, "name": "OrderItem",
                "plural": "OrderItems", "package_ID": PACKAGE_ID }),
    )
    .await;
    create(
        &store,
        "FieldSet",
        json!({ "IsActiveEntity": true, "entity_ID": ORDER_ID, "name": "amount",
                "label": "Amount", "type": "Edm.Decimal", "precision": 15, "scale": 2 }),
    )
    .await;
    create(
        &store,
        "RelationSet",
        json!({ "ID":RELATION_ID,
                "IsActiveEntity": true, "name": "order_items", "owned": true,
                "one_ID": ORDER_ID, "source_field": "_Items",
                "many_ID": ITEM_ID, "target_field": "order" }),
    )
    .await;
    create(
        &store,
        "AppSet",
        json!({ "IsActiveEntity": true, "name": "orders", "title": "Orders",
                "main_entity_ID": ORDER_ID, "package_ID": PACKAGE_ID }),
    )
    .await;
    let mut config = ServerConfig::defaults(ServerMode::Designer);
    config.export_dir = "sample-data".into();
    let state = AppState::new(store, Arc::new(config), DataDictionary::new());
    state.reload();
    let report = export_dictionary(&state).await?;
    println!("-> {:#?}", report);
    Ok(())
}
