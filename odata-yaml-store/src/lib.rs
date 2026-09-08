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
    use crate::datastore::Navigation;

    use super::*;

    use odata::query::{ODataQuery, SortDirection};
    use serde_json::json;
    use tempfile::TempDir;
    use uuid::Uuid;

    const ORDERS: &str = "Orders";
    const ORDER_ITEMS: &str = "OrderItems";
    const CUSTOMERS: &str = "Customers";

    fn make_store() -> (TempDir, impl DraftDataStore) {
        let dir = TempDir::new().unwrap();
        let store = YamlStore::open(dir.path()).unwrap();
        (dir, store)
    }

    fn to_uuid(n: usize) -> Uuid {
        Uuid::parse_str(&format!("00000000-0000-0000-0000-{:012X}", n)).expect("UUID mismatch")
    }

    fn setup_data(store: &mut impl DraftDataStore) -> anyhow::Result<()> {
        // die 'single' Navigations nutze jeweils
        // das Feld <from_field>_ID auslesen, die ID verwenden um den Datensatz unter
        // <from_field> einzusetzen.
        //
        let navigations = vec![
            Navigation {
                single: false,
                owning: true,
                from_set: Some(ORDERS.into()),
                from_field: Some("_Items".into()),
                to_set: Some(ORDER_ITEMS.into()),
                to_field: Some("Order_ID".into()),
            },
            Navigation {
                single: true,
                owning: false,
                from_set: Some(ORDERS.into()),
                from_field: Some("Customer".into()),
                to_set: Some(CUSTOMERS.into()),
                to_field: Some("ID".into()),
            },
            Navigation {
                single: true,
                owning: true,
                from_set: Some(ORDERS.into()),
                from_field: Some("DraftAdministrativeData".into()),
                to_set: Some("DraftAdministrativeData".into()),
                to_field: Some("ID".into()),
            },
        ];
        store.set_navigations(navigations);

        let cust_mueller = to_uuid(100);
        let cust_schmidt = to_uuid(101);

        store.create_entity(
            CUSTOMERS,
            &json!({"ID": cust_mueller, "IsActiveEntity":true, "Name": "Müller GmbH"}),
        )?;
        // store.draft_activate(CUSTOMERS, &EntityKey::new(cust_mueller, false))?;

        store.create_entity(
            CUSTOMERS,
            &json!({"ID": cust_schmidt, "IsActiveEntity":true, "Name": "Schmidt AG"}),
        )?;
        // store.draft_activate(CUSTOMERS, &EntityKey::new(cust_schmidt, false))?;

        store.create_entity(
        ORDERS,
        &json!({ "ID": to_uuid(1), "IsActiveEntity":true, "Customer_ID": cust_mueller, "Amount": 150, "Status": "New" }),
    )?;
        store.create_entity(
        ORDERS,
        &json!({ "ID": to_uuid(2), "IsActiveEntity":true, "Customer_ID": cust_schmidt, "Amount": 480, "Status": "Released" }),
    )?;
        store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(10), "IsActiveEntity":true, "Order_ID": to_uuid(1), "Material": "M-100", "Qty": 5 }),
        )?;
        store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(11), "IsActiveEntity":true, "Order_ID": to_uuid(1), "Material": "M-200", "Qty": 2 }),
        )?;
        let _ = store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(12), "IsActiveEntity":true, "Order_ID": to_uuid(2), "Material": "M-200", "Qty": 2 }),
        );
        Ok(())
    }
    #[test]
    fn create() {
        // Entites sind automatisch im Draft Modus, wenn sie angelegt werden.
        // Das bedeutet, dass `IsActiveEntity` beim Erstellen auf `false` gesetzt wird.
        // Nur so können sie dann auch wieder gelesen werden.

        let id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let (_dir, store) = make_store();
        let data = json!({ "ID": id, "CustomerName": "Mueller GmbH", "Amount": 150 });
        store.create_entity(ORDERS, &data).unwrap();

        let key = EntityKey::new(id, false);
        let q = ODataQuery::new();
        let read = store.read_entity(ORDERS, &key, &q).unwrap();
        assert_eq!(read["CustomerName"], "Mueller GmbH");
    }

    #[test]
    fn initial_data() -> anyhow::Result<()> {
        let (dir, mut store) = make_store();
        setup_data(&mut store)?;
        check_entity_dirs(&dir, ORDERS, 2, 0)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 3, 0)?;
        Ok(())
    }

    fn check_entity_dirs(
        dir: &TempDir,
        name: &str,
        active_count: usize,
        draft_count: usize,
    ) -> anyhow::Result<()> {
        let entity_dir = dir.path().join(name);
        assert!(entity_dir.exists());
        assert!(entity_dir.is_dir());
        assert_eq!(2, entity_dir.read_dir()?.count());
        check_state_dirs(entity_dir, active_count, draft_count)?;
        Ok(())
    }

    fn check_state_dirs(
        entity_dir: std::path::PathBuf,
        active_count: usize,
        draft_count: usize,
    ) -> anyhow::Result<()> {
        let active_dir = entity_dir.join("active");
        let draft_dir = entity_dir.join("draft");
        assert!(active_dir.exists(), "dir {}", active_dir.display());
        assert_eq!(
            active_dir.read_dir()?.count(),
            active_count,
            "{}",
            active_dir.display()
        );
        assert!(draft_dir.exists(), "dir {}", draft_dir.display());
        assert_eq!(
            draft_dir.read_dir()?.count(),
            draft_count,
            "{}",
            draft_dir.display()
        );
        Ok(())
    }

    #[test]
    fn edit_full_cycle() -> anyhow::Result<()> {
        let (dir, mut store) = make_store();
        setup_data(&mut store).unwrap();
        let key = &EntityKey {
            id: to_uuid(2),
            is_active_entity: true,
        };
        store.draft_edit(ORDERS, key)?;
        let rec = store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(13), "IsActiveEntity":false, "Order_ID": to_uuid(2), "Material": "M-300", "Qty": 42}),
        )?;
        assert!(!rec.is_active_entity());
        let rec = store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(14), "IsActiveEntity":false, "Order_ID": to_uuid(2), "Material": "M-400", "Qty": 4}),
        )?;
        assert!(!rec.is_active_entity());
        check_entity_dirs(&dir, ORDERS, 2, 1)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 3, 3)?;
        store.draft_activate(ORDERS, &key.sibling())?;
        check_entity_dirs(&dir, ORDERS, 2, 0)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 5, 0)?;

        let query = ODataQuery::new().expand("_Items");
        let r = store.read_entity(ORDERS, &key, &query)?;
        let items = r.get("_Items").unwrap();
        assert_eq!(items.as_array().unwrap().len(), 3);

        // versuche einen Item alleine zu editieren. Sollte nicht gehen
        //
        let item_key = EntityKey {
            id: to_uuid(13),
            is_active_entity: true,
        };
        let r = store.draft_edit(ORDER_ITEMS, &item_key);
        assert!(r.is_err());

        Ok(())
    }

    #[test]
    fn delete_parent() -> anyhow::Result<()> {
        let (dir, mut store) = make_store();
        setup_data(&mut store).unwrap();
        let key = &EntityKey {
            id: to_uuid(2),
            is_active_entity: true,
        };
        store.delete_entity(ORDERS, key)?;
        check_entity_dirs(&dir, ORDERS, 1, 0)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 2, 0)?;
        Ok(())
    }

    #[test]
    fn delete_parent_draft() -> anyhow::Result<()> {
        let (dir, mut store) = make_store();
        setup_data(&mut store).unwrap();
        let key = &EntityKey {
            id: to_uuid(2),
            is_active_entity: true,
        };
        store.draft_edit(ORDERS, key)?;
        let _rec = store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(13), "IsActiveEntity":false, "Order_ID": to_uuid(2), "Material": "M-300", "Qty": 42}),
        )?;
        check_entity_dirs(&dir, ORDERS, 2, 1)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 3, 2)?;
        store.delete_entity(ORDERS, &key.sibling())?;
        check_entity_dirs(&dir, ORDERS, 1, 0)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 2, 0)?;
        Ok(())
    }

    #[test]
    fn delete_child() -> anyhow::Result<()> {
        let (dir, mut store) = make_store();
        setup_data(&mut store).unwrap();
        let key = &EntityKey {
            id: to_uuid(12),
            is_active_entity: true,
        };
        store.delete_entity(ORDER_ITEMS, key)?;
        check_entity_dirs(&dir, ORDERS, 2, 0)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 2, 0)?;
        Ok(())
    }

    #[test]
    fn delete_child_draft() -> anyhow::Result<()> {
        let (dir, mut store) = make_store();
        setup_data(&mut store).unwrap();
        let parent_key = EntityKey::new_active(to_uuid(2));
        let key = &EntityKey::new_active(to_uuid(12));
        store.draft_edit(ORDERS, &parent_key)?;
        check_entity_dirs(&dir, ORDERS, 2, 1)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 3, 1)?;
        let _rec = store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(13), "IsActiveEntity":false, "Order_ID": to_uuid(2), "Material": "M-300", "Qty": 42}),
        )?;
        check_entity_dirs(&dir, ORDERS, 2, 1)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 3, 2)?;
        store.delete_entity(ORDER_ITEMS, &key.sibling())?;
        check_entity_dirs(&dir, ORDERS, 2, 1)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 2, 1)?;
        Ok(())
    }

    #[test]
    fn patch_entity_merges_fields() {
        let (_dir, store) = make_store();
        let id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let data = json!({ "ID": id, "Status": "New", "Amount": 100 });
        store.create_entity(ORDERS, &data).unwrap();

        let key = EntityKey::new(id, false);
        store
            .patch_entity(ORDERS, &key, &json!({ "Status": "Released" }))
            .unwrap();

        let q = ODataQuery::new();
        let read = store.read_entity(ORDERS, &key, &q).unwrap();
        assert_eq!(read["Status"], "Released");
        assert_eq!(read["Amount"], 100); // unveraendert
    }

    #[test]
    fn get_collection_with_filter_and_orderby() {
        let (_dir, store) = make_store();
        store
            .create_entity(
                ORDERS,
                &json!({ "ID": Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(), "Amount": 50, "Status": "New" }),
            )
            .unwrap();
        store
            .create_entity(
                ORDERS,
                &json!({ "ID": Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(), "Amount": 200, "Status": "Released" }),
            )
            .unwrap();
        store
            .create_entity(
                ORDERS,
                &json!({ "ID": Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap(), "Amount": 120, "Status": "Released" }),
            )
            .unwrap();

        let query = ODataQuery::new().order_by("Amount", SortDirection::Desc);

        let result = store.get_collection(ORDERS, &query).unwrap();
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
    fn get_collection_applies_skip_and_top() {
        let (_dir, store) = make_store();
        for (n, id) in [
            "00000000-0000-0000-0000-000000000001",
            "00000000-0000-0000-0000-000000000002",
            "00000000-0000-0000-0000-000000000003",
        ]
        .iter()
        .enumerate()
        {
            store
                .create_entity(
                    ORDERS,
                    &json!({ "ID": Uuid::parse_str(id).unwrap(), "Amount": (n as i64 + 1) * 10 }),
                )
                .unwrap();
        }

        let mut query = ODataQuery::new().order_by("Amount", SortDirection::Asc);
        query.skip = Some(1);
        query.top = Some(1);

        let result = store.get_collection(ORDERS, &query).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].get("Amount"), Some(json!(20)));
        // $count ignoriert die Seitenaufteilung
        assert_eq!(store.count(ORDERS, &query), 3);
    }

    #[test]
    fn draft_edit_activate_roundtrip() {
        let (_dir, store) = make_store();
        store
            .create_entity(
                ORDERS,
                &json!({ "ID": "00000000-0000-0000-0000-000000000001",
                         "IsActiveEntity": true,
                         "Status": "Released" }),
            )
            .unwrap();

        let active_key = EntityKey::new_active("00000000-0000-0000-0000-000000000001");

        let draft = store.draft_edit(ORDERS, &active_key).unwrap();
        assert_eq!(draft["IsActiveEntity"], false);

        // Draft bearbeiten
        let draft_key = active_key.sibling();
        store
            .patch_entity(ORDERS, &draft_key, &json!({ "Status": "InProgress" }))
            .unwrap();

        // Aktivieren
        let activated = store.draft_activate(ORDERS, &draft_key).unwrap();
        assert_eq!(activated["Status"], "InProgress");
        assert_eq!(activated["IsActiveEntity"], true);

        // let debug: &dyn DataStoreDebugExt = &store;
        // let remaining_drafts: Vec<_> = debug
        // .get_records(ORDERS)
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
                    ORDERS,
                    &json!({ "ID": "00000000-0000-0000-0000-000000000001",
                                                    "IsActiveEntity": true,
                                                    "Amount": 42 }),
                )
                .unwrap();
        }

        let reopened = YamlStore::open(dir.path()).unwrap();
        let key = EntityKey::new_active("00000000-0000-0000-0000-000000000001");
        let q = ODataQuery::new();
        let read = reopened.read_entity(ORDERS, &key, &q).unwrap();
        assert_eq!(read["Amount"], 42);
    }
    #[test]
    fn expand_draft_metadata() -> anyhow::Result<()> {
        let (_dir, mut store) = make_store();
        setup_data(&mut store).unwrap();
        let key = EntityKey::new_active(to_uuid(1));
        store.draft_edit(ORDERS, &key)?;
        let query = ODataQuery::new().expand("DraftAdministrativeData");
        let r = store.read_entity(ORDERS, &key.sibling(), &query)?;
        assert!(r.draft_admin_data().is_some());
        Ok(())
    }
    #[test]
    fn expand() -> anyhow::Result<()> {
        let (_dir, mut store) = make_store();
        setup_data(&mut store)?;

        let q = ODataQuery::new()
            .expand("_Items")
            .expand("Customer")
            .order_by("Amount", SortDirection::Asc);
        let result = store.get_collection(ORDERS, &q)?;
        match result.as_slice() {
            [r, _] => {
                let items = r.get("_Items").unwrap();
                assert_eq!(items.as_array().map(|a| a.len()), Some(2));
                let customer = r.get("Customer").unwrap();
                assert_eq!(customer["Name"], json!("Müller GmbH"));
            }
            x => panic!("selektion hat nicht geklappt: {:#?}", x),
        }

        let key = EntityKey::new_active(to_uuid(1));
        let r = store.read_entity(ORDERS, &key, &q)?;
        assert_eq!(
            r.get("_Items").unwrap().as_array().map(|a| a.len()),
            Some(2)
        );
        assert_eq!(r.get("Customer").unwrap()["Name"], json!("Müller GmbH"));
        Ok(())
    }
}
