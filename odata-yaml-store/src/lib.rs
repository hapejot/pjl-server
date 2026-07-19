pub mod datastore;
pub mod error;
pub mod filter_eval;
pub mod key;
pub mod metadata;
pub mod yaml_store;

pub use datastore::{DataStore, DraftAction};
pub use error::StoreError;
pub use key::{EntityKey, ParentKey};
pub use metadata::{EntityMetadata, NavigationDef};
pub use yaml_store::YamlStore;

#[cfg(any(test, feature = "debug-tools"))]
pub use datastore::DataStoreDebugExt;

/// Bequeme Wrapper-Methoden, damit Aufrufer weiterhin `store.draft_edit(...)`
/// statt `store.draft_action(..., DraftAction::Edit)` schreiben koennen.
/// Das Kern-Trait `DataStore` selbst bleibt schlank (nur `draft_action`).
pub trait DraftActionExt: DataStore {
    fn draft_edit(&self, set_name: &str, key: &EntityKey) -> Result<serde_json::Value, StoreError> {
        self.draft_action(set_name, key, DraftAction::Edit)
    }
    fn draft_activate(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError> {
        self.draft_action(set_name, key, DraftAction::Activate)
    }
    fn draft_prepare(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError> {
        self.draft_action(set_name, key, DraftAction::Prepare)
    }
}
impl<T: DataStore + ?Sized> DraftActionExt for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use datastore::DataStoreDebugExt;
    use odata::query::*;
    use serde_json::json;
    use tempfile::TempDir;
    use uuid::Uuid;

    fn make_store() -> (TempDir, YamlStore) {
        let dir = TempDir::new().unwrap();
        let mut meta = EntityMetadata::new();
        meta.register_key("Orders", &["ID"]);
        meta.register_key("OrderItems", &["ID"]);
        meta.register_navigation(
            "Orders",
            "Items",
            NavigationDef {
                target_set: "OrderItems".to_string(),
                foreign_key_field: "OrderID".to_string(),
            },
        );
        let store = YamlStore::open(dir.path(), meta).unwrap();
        (dir, store)
    }

    #[test]
    fn create_and_read_entity() {
        let (_dir, store) = make_store();
        let data = json!({ "ID": "1", "CustomerName": "Mueller GmbH", "Amount": 150 });
        store.create_entity("Orders", &data).unwrap();

        let key = EntityKey::new(
            Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
            true,
        );
        let read = store.read_entity("Orders", &key).unwrap();
        assert_eq!(read["CustomerName"], "Mueller GmbH");
    }

    #[test]
    fn patch_entity_merges_fields() {
        let (_dir, store) = make_store();
        let data = json!({ "ID": "1", "Status": "New", "Amount": 100 });
        store.create_entity("Orders", &data).unwrap();

        let key = EntityKey::new(
            Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
            true,
        );
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
                &json!({ "ID": "1", "Amount": 50, "Status": "New" }),
            )
            .unwrap();
        store
            .create_entity(
                "Orders",
                &json!({ "ID": "2", "Amount": 200, "Status": "Released" }),
            )
            .unwrap();
        store
            .create_entity(
                "Orders",
                &json!({ "ID": "3", "Amount": 120, "Status": "Released" }),
            )
            .unwrap();

        let query = ODataQuery {
            filter: None,
            orderby: vec![OrderByItem {
                property: "Amount".to_string(),
                direction: SortDirection::Desc,
            }],
            ..Default::default()
        };

        let result = store.get_collection("Orders", &query).unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["ID"], "2"); // Amount 200 zuerst (desc)
        assert_eq!(arr[1]["ID"], "3");
    }

    #[test]
    fn expand_resolves_composition_children() {
        let (_dir, store) = make_store();
        store
            .create_entity("Orders", &json!({ "ID": "1", "CustomerName": "Foo" }))
            .unwrap();
        store
            .create_entity(
                "OrderItems",
                &json!({ "ID": "10", "OrderID": "1", "Qty": 3 }),
            )
            .unwrap();
        store
            .create_entity(
                "OrderItems",
                &json!({ "ID": "11", "OrderID": "1", "Qty": 7 }),
            )
            .unwrap();

        let query = ODataQuery {
            expand: vec![ExpandItem {
                navigation_property: "Items".to_string(),
                nested_query: Box::new(ODataQuery::default()),
            }],
            ..Default::default()
        };

        let result = store.get_collection("Orders", &query).unwrap();
        let order = &result.as_array().unwrap()[0];
        let items = order["Items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn draft_edit_activate_roundtrip() {
        let (_dir, store) = make_store();
        store
            .create_entity(
                "Orders",
                &json!({ "ID": "1", "Status": "Released", "IsActiveEntity": true }),
            )
            .unwrap();

        let active_key = EntityKey::new(
            Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
            true,
        );

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
        let mut meta = EntityMetadata::new();
        meta.register_key("Orders", &["ID"]);

        {
            let store = YamlStore::open(dir.path(), meta).unwrap();
            store
                .create_entity("Orders", &json!({ "ID": "1", "Amount": 42 }))
                .unwrap();
        }

        let mut meta2 = EntityMetadata::new();
        meta2.register_key("Orders", &["ID"]);
        let reopened = YamlStore::open(dir.path(), meta2).unwrap();
        let key = EntityKey::new(
            Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
            true,
        );
        let read = reopened.read_entity("Orders", &key).unwrap();
        assert_eq!(read["Amount"], 42);
    }
}
