pub mod datastore;
pub mod error;
pub mod filter_eval;
pub mod key;
pub mod yaml_store;

pub use datastore::{DataStore, DraftAction};
pub use error::StoreError;
pub use key::EntityKey;
pub use yaml_store::YamlStore;

#[cfg(any(test, feature = "debug-tools"))]
pub use datastore::DataStoreDebugExt;

pub trait DraftDataStore: DataStore {
    fn draft_edit(&self, set_name: &str, key: &EntityKey) -> Result<serde_json::Value, StoreError>;
    fn draft_activate(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError>;
    fn draft_prepare(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    use odata::query::*;
    use serde_json::json;
    use tempfile::TempDir;
    use uuid::Uuid;

    fn make_store() -> (TempDir, impl DraftDataStore) {
        let dir = TempDir::new().unwrap();
        let store = YamlStore::open(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn create_and_read_entity() {
        // Entites sind automatisch im Draft Modus, wenn sie angelegt werden.
        // Das bedeutet, dass `IsActiveEntity` beim Erstellen auf `false` gesetzt wird.
        // Nur so können sie dann auch wieder gelesen werden.

        let id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let (_dir, store) = make_store();
        let data = json!({ "ID": id, "CustomerName": "Mueller GmbH", "Amount": 150 });
        store.create_entity("Orders", &data).unwrap();

        let key = EntityKey::new(id, false);
        let read = store.read_entity("Orders", &key).unwrap();
        assert_eq!(read["CustomerName"], "Mueller GmbH");
    }

    #[test]
    fn patch_entity_merges_fields() {
        let (_dir, store) = make_store();
        let id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let data = json!({ "ID": id, "Status": "New", "Amount": 100 });
        store.create_entity("Orders", &data).unwrap();

        let key = EntityKey::new(id, false);
        store
            .patch_entity("Orders", &key, &json!({ "Status": "Released" }))
            .unwrap();

        let read = store.read_entity("Orders", &key).unwrap();
        assert_eq!(read["Status"], "Released");
        assert_eq!(read["Amount"], 100); // unveraendert
    }

    #[test]
    fn get_collection_with_filter_and_orderby() {
        let (_dir, store) = make_store();
        store
            .create_entity(
                "Orders",
                &json!({ "ID": Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(), "Amount": 50, "Status": "New" }),
            )
            .unwrap();
        store
            .create_entity(
                "Orders",
                &json!({ "ID": Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(), "Amount": 200, "Status": "Released" }),
            )
            .unwrap();
        store
            .create_entity(
                "Orders",
                &json!({ "ID": Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap(), "Amount": 120, "Status": "Released" }),
            )
            .unwrap();

        let query = ODataQuery::new().order_by("Amount", SortDirection::Desc);

        let result = store.get_collection("Orders", &query).unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(
            result[0].get("ID"),
            Some(json!("00000000-0000-0000-0000-000000000002"))
        ); // Amount 200 zuerst (desc)
        assert_eq!(
            result[1].get("ID"),
            Some(json!("00000000-0000-0000-0000-000000000003"))
        );
        assert_eq!(
            result[2].get("ID"),
            Some(json!("00000000-0000-0000-0000-000000000001"))
        );
    }

    #[test]
    fn draft_edit_activate_roundtrip() {
        let (_dir, store) = make_store();
        store
            .create_entity(
                "Orders",
                &json!({ "ID": "00000000-0000-0000-0000-000000000001",
                         "IsActiveEntity": true,
                         "Status": "Released" }),
            )
            .unwrap();

        let active_key = EntityKey::new_active("00000000-0000-0000-0000-000000000001");

        let draft = store.draft_edit("Orders", &active_key).unwrap();
        assert_eq!(draft["IsActiveEntity"], false);

        // Draft bearbeiten
        let draft_key = active_key.sibling();
        store
            .patch_entity("Orders", &draft_key, &json!({ "Status": "InProgress" }))
            .unwrap();

        // Aktivieren
        let activated = store.draft_activate("Orders", &draft_key).unwrap();
        assert_eq!(activated["Status"], "InProgress");
        assert_eq!(activated["IsActiveEntity"], true);

        // let debug: &dyn DataStoreDebugExt = &store;
        // let remaining_drafts: Vec<_> = debug
        // .get_records("Orders")
        // .into_iter()
        // .filter(|r| r["IsActiveEntity"] == false)
        // .collect();
        // assert!(remaining_drafts.is_empty());
    }

    #[test]
    fn persists_across_reopen() {
        let dir = TempDir::new().unwrap();

        {
            let store = YamlStore::open(dir.path()).unwrap();
            store
                .create_entity(
                    "Orders",
                    &json!({ "ID": "00000000-0000-0000-0000-000000000001",
                                                    "IsActiveEntity": true,
                                                    "Amount": 42 }),
                )
                .unwrap();
        }

        let reopened = YamlStore::open(dir.path()).unwrap();
        let key = EntityKey::new_active("00000000-0000-0000-0000-000000000001");
        let read = reopened.read_entity("Orders", &key).unwrap();
        assert_eq!(read["Amount"], 42);
    }
    #[test]
    fn expand() {
        let (_dir, mut store) = make_store();
        store
            .navigation(false)
            .from("Orders", "_Items")
            .to("Items", "OrderID");
        store
            .navigation(true)
            .from("Orders", "Customer")
            .to("Customers", "ID");
        store
            .create_entity(
                "Customers",
                &json!({
                            "ID":"00000000-0000-0000-0000-000000000005",
                            "Name": "ACME Inc."}),
            )
            .unwrap();
        store
            .create_entity(
                "Orders",
                &json!({ "ID": "00000000-0000-0000-0000-000000000001",
                "Customer": "00000000-0000-0000-0000-000000000005",
                "Status": "Released" }),
            )
            .unwrap();
        store.create_entity("Items", &json!({"ID": Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(),
                                            "Pos": "00010",
                                            "OrderID": Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()})).unwrap();
        store.create_entity("Items", &json!({"ID": Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap(),
                                            "Pos": "00020",
                                            "OrderID": Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()})).unwrap();
        store.create_entity("Items", &json!({"ID": Uuid::parse_str("00000000-0000-0000-0000-000000000004").unwrap(),
                                            "Pos": "00030",
                                            "OrderID": Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()})).unwrap();

        let q = ODataQuery::new().expand("_Items").expand("Customer");
        let result = store.get_collection("Orders", &q).unwrap();
        match result.as_slice() {
            [r] => {
                let x = r.get("_Items").unwrap();
                assert!(x.is_array());
                let x = r.get("Customer").unwrap();
                assert!(x.is_object());
            }
            _ => panic!(),
        }
        let _r = store
            .read_entity("Orders", &EntityKey::new_active(""))
            .unwrap();
    }
}
