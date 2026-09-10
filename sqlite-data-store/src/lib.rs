//! SQLite-Implementierung des in `data-store` definierten Interfaces.
//!
//! Die Abstraktion (Traits, `DataRecord`, `EntityKey`, ...) lebt in `data-store`;
//! dieses Crate liefert die SQLite-spezifische Ablage.

// Re-export der abstrakten Schnittstelle, damit Nutzer nur ein Crate brauchen.
pub use data_store::{
    DataRecord, DataStore, DraftAction, DraftAdministrativeData, DraftDataStore, DraftMessage,
    EntityKey, Navigation, StoreError,
};

pub mod sqlite;
pub use sqlite::SqliteStore;

#[cfg(test)]
mod tests {
    use super::*;
    use odata::query::{ODataQuery, SortDirection};
    use serde_json::json;
    use tempfile::TempDir;
    use uuid::Uuid;

    const ORDERS: &str = "Orders";
    const ORDER_ITEMS: &str = "OrderItems";
    const CUSTOMERS: &str = "Customers";

    fn make_store() -> (TempDir, SqliteStore) {
        let dir = TempDir::new().unwrap();
        let store = SqliteStore::open(dir.path()).unwrap();
        (dir, store)
    }

    fn to_uuid(n: usize) -> Uuid {
        Uuid::parse_str(&format!("00000000-0000-0000-0000-{:012X}", n)).expect("UUID mismatch")
    }

    fn navigations() -> Vec<Navigation> {
        vec![
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
        ]
    }

    fn setup_data(store: &SqliteStore) -> anyhow::Result<()> {
        store.set_navigations(navigations());

        let cust_mueller = to_uuid(100);
        let cust_schmidt = to_uuid(101);

        store.create_entity(
            CUSTOMERS,
            &json!({"ID": cust_mueller, "IsActiveEntity": true, "Name": "Müller GmbH"}),
        )?;
        store.create_entity(
            CUSTOMERS,
            &json!({"ID": cust_schmidt, "IsActiveEntity": true, "Name": "Schmidt AG"}),
        )?;
        store.create_entity(
            ORDERS,
            &json!({ "ID": to_uuid(1), "IsActiveEntity": true, "Customer_ID": cust_mueller, "Amount": 150, "Status": "New" }),
        )?;
        store.create_entity(
            ORDERS,
            &json!({ "ID": to_uuid(2), "IsActiveEntity": true, "Customer_ID": cust_schmidt, "Amount": 480, "Status": "Released" }),
        )?;
        store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(10), "IsActiveEntity": true, "Order_ID": to_uuid(1), "Material": "M-100", "Qty": 5 }),
        )?;
        store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(11), "IsActiveEntity": true, "Order_ID": to_uuid(1), "Material": "M-200", "Qty": 2 }),
        )?;
        store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(12), "IsActiveEntity": true, "Order_ID": to_uuid(2), "Material": "M-200", "Qty": 2 }),
        )?;
        Ok(())
    }

    /// Pendant zu `check_entity_dirs` im YAML-Store: zaehlt die abgelegten Saetze.
    fn check_counts(store: &SqliteStore, name: &str, active_count: usize, draft_count: usize) {
        assert_eq!(
            store.count_records(name, true),
            active_count,
            "{name}: active"
        );
        assert_eq!(
            store.count_records(name, false),
            draft_count,
            "{name}: draft"
        );
    }

    #[test]
    fn create() {
        let id = to_uuid(1);
        let (_dir, store) = make_store();
        store
            .create_entity(
                ORDERS,
                &json!({ "ID": id, "CustomerName": "Mueller GmbH", "Amount": 150 }),
            )
            .unwrap();

        let key = EntityKey::new(id, false);
        let read = store.read_entity(ORDERS, &key, &ODataQuery::new()).unwrap();
        assert_eq!(read["CustomerName"], "Mueller GmbH");
    }

    #[test]
    fn initial_data() -> anyhow::Result<()> {
        let (_dir, store) = make_store();
        setup_data(&store)?;
        check_counts(&store, ORDERS, 2, 0);
        check_counts(&store, CUSTOMERS, 2, 0);
        check_counts(&store, ORDER_ITEMS, 3, 0);
        Ok(())
    }

    #[test]
    fn edit_full_cycle() -> anyhow::Result<()> {
        let (_dir, store) = make_store();
        setup_data(&store)?;
        let key = EntityKey::new_active(to_uuid(2));

        store.draft_edit(ORDERS, &key)?;
        let rec = store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(13), "IsActiveEntity": false, "Order_ID": to_uuid(2), "Material": "M-300", "Qty": 42}),
        )?;
        assert!(!rec.is_active_entity());
        let rec = store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(14), "IsActiveEntity": false, "Order_ID": to_uuid(2), "Material": "M-400", "Qty": 4}),
        )?;
        assert!(!rec.is_active_entity());

        check_counts(&store, ORDERS, 2, 1);
        check_counts(&store, CUSTOMERS, 2, 0);
        check_counts(&store, ORDER_ITEMS, 3, 3);

        store.draft_activate(ORDERS, &key.sibling())?;
        check_counts(&store, ORDERS, 2, 0);
        check_counts(&store, CUSTOMERS, 2, 0);
        check_counts(&store, ORDER_ITEMS, 5, 0);

        let query = ODataQuery::new().expand("_Items");
        let r = store.read_entity(ORDERS, &key, &query)?;
        assert_eq!(r.get("_Items").unwrap().as_array().unwrap().len(), 3);

        // Ein Item alleine editieren geht nicht - der Parent ist nicht im Draft.
        let item_key = EntityKey::new_active(to_uuid(13));
        assert!(store.draft_edit(ORDER_ITEMS, &item_key).is_err());
        Ok(())
    }

    #[test]
    fn delete_parent() -> anyhow::Result<()> {
        let (_dir, store) = make_store();
        setup_data(&store)?;
        store.delete_entity(ORDERS, &EntityKey::new_active(to_uuid(2)))?;
        check_counts(&store, ORDERS, 1, 0);
        check_counts(&store, CUSTOMERS, 2, 0);
        check_counts(&store, ORDER_ITEMS, 2, 0);
        Ok(())
    }

    #[test]
    fn delete_parent_draft() -> anyhow::Result<()> {
        let (_dir, store) = make_store();
        setup_data(&store)?;
        let key = EntityKey::new_active(to_uuid(2));
        store.draft_edit(ORDERS, &key)?;
        store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(13), "IsActiveEntity": false, "Order_ID": to_uuid(2), "Material": "M-300", "Qty": 42}),
        )?;
        check_counts(&store, ORDERS, 2, 1);
        check_counts(&store, ORDER_ITEMS, 3, 2);

        store.delete_entity(ORDERS, &key.sibling())?;
        check_counts(&store, ORDERS, 1, 0);
        check_counts(&store, CUSTOMERS, 2, 0);
        check_counts(&store, ORDER_ITEMS, 2, 0);
        Ok(())
    }

    #[test]
    fn delete_child() -> anyhow::Result<()> {
        let (_dir, store) = make_store();
        setup_data(&store)?;
        store.delete_entity(ORDER_ITEMS, &EntityKey::new_active(to_uuid(12)))?;
        check_counts(&store, ORDERS, 2, 0);
        check_counts(&store, CUSTOMERS, 2, 0);
        check_counts(&store, ORDER_ITEMS, 2, 0);
        Ok(())
    }

    #[test]
    fn delete_child_draft() -> anyhow::Result<()> {
        let (_dir, store) = make_store();
        setup_data(&store)?;
        let parent_key = EntityKey::new_active(to_uuid(2));
        let key = EntityKey::new_active(to_uuid(12));

        store.draft_edit(ORDERS, &parent_key)?;
        check_counts(&store, ORDERS, 2, 1);
        check_counts(&store, ORDER_ITEMS, 3, 1);

        store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(13), "IsActiveEntity": false, "Order_ID": to_uuid(2), "Material": "M-300", "Qty": 42}),
        )?;
        check_counts(&store, ORDER_ITEMS, 3, 2);

        store.delete_entity(ORDER_ITEMS, &key.sibling())?;
        check_counts(&store, ORDERS, 2, 1);
        check_counts(&store, CUSTOMERS, 2, 0);
        check_counts(&store, ORDER_ITEMS, 2, 1);
        Ok(())
    }

    #[test]
    fn delete_entity_tree_only_keeps_sibling() -> anyhow::Result<()> {
        let (_dir, store) = make_store();
        setup_data(&store)?;
        let parent_key = EntityKey::new_active(to_uuid(2));
        store.draft_edit(ORDERS, &parent_key)?;
        check_counts(&store, ORDER_ITEMS, 3, 1);

        // nur den Draft-Zweig loeschen - der aktive Satz bleibt erhalten
        store.delete_entity_tree_only(ORDER_ITEMS, &EntityKey::new(to_uuid(12), false))?;
        check_counts(&store, ORDER_ITEMS, 3, 0);
        Ok(())
    }

    /// Regression: beim Aktivieren eines Drafts darf ein Kind, das es *nur*
    /// aktiv gibt, nicht angefasst werden. Zuvor lieferte die Kindsuche wegen
    /// der Draft-Ueberdeckung den aktiven Satz zurueck, der dann im Zuge der
    /// "Aktivierung" geloescht wurde.
    #[test]
    fn draft_activate_keeps_active_only_child() -> anyhow::Result<()> {
        let (_dir, store) = make_store();
        setup_data(&store)?;
        let parent = EntityKey::new_active(to_uuid(2));
        store.draft_edit(ORDERS, &parent)?;

        // Kind direkt aktiv anlegen - es hat damit keinen Draft-Zwilling.
        store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(20), "IsActiveEntity": true, "Order_ID": to_uuid(2), "Material": "M-900", "Qty": 1}),
        )?;
        check_counts(&store, ORDER_ITEMS, 4, 1);

        store.draft_activate(ORDERS, &parent.sibling())?;

        // 10, 11, 12 (aktiviert) und 20 (unveraendert aktiv)
        check_counts(&store, ORDER_ITEMS, 4, 0);
        let survivor = store.read_entity(
            ORDER_ITEMS,
            &EntityKey::new_active(to_uuid(20)),
            &ODataQuery::new(),
        )?;
        assert_eq!(survivor["Material"], json!("M-900"));
        Ok(())
    }

    /// Regression: das Loeschen eines aktiven Baums muss den aktiven Kindsatz
    /// treffen - nicht den Draft, der ihn in Collections ueberdeckt.
    #[test]
    fn delete_active_parent_removes_active_children() -> anyhow::Result<()> {
        let (_dir, store) = make_store();
        setup_data(&store)?;
        let parent = EntityKey::new_active(to_uuid(2));
        store.draft_edit(ORDERS, &parent)?;
        check_counts(&store, ORDER_ITEMS, 3, 1);

        store.delete_entity(ORDERS, &parent)?;

        // Der aktive Kindsatz 12 ist mit dem aktiven Parent verschwunden und
        // bleibt nicht als Waise zurueck.
        check_counts(&store, ORDER_ITEMS, 2, 1);
        assert!(
            store
                .read_entity(
                    ORDER_ITEMS,
                    &EntityKey::new_active(to_uuid(12)),
                    &ODataQuery::new()
                )
                .is_err()
        );
        Ok(())
    }

    /// `draft_activate` arbeitet nur auf Draft-Schluesseln.
    #[test]
    fn draft_activate_rejects_active_key() -> anyhow::Result<()> {
        let (_dir, store) = make_store();
        setup_data(&store)?;
        assert!(
            store
                .draft_activate(ORDERS, &EntityKey::new_active(to_uuid(2)))
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn patch_entity_merges_fields() {
        let (_dir, store) = make_store();
        let id = to_uuid(1);
        store
            .create_entity(ORDERS, &json!({ "ID": id, "Status": "New", "Amount": 100 }))
            .unwrap();

        let key = EntityKey::new(id, false);
        store
            .patch_entity(ORDERS, &key, &json!({ "Status": "Released" }))
            .unwrap();

        let read = store.read_entity(ORDERS, &key, &ODataQuery::new()).unwrap();
        assert_eq!(read["Status"], "Released");
        assert_eq!(read["Amount"], 100);
    }

    #[test]
    fn get_collection_with_filter_and_orderby() {
        let (_dir, store) = make_store();
        for (n, amount) in [50, 200, 120].iter().enumerate() {
            store
                .create_entity(
                    ORDERS,
                    &json!({ "ID": to_uuid(n + 1), "Amount": amount, "Status": "New" }),
                )
                .unwrap();
        }

        let query = ODataQuery::new().order_by("Amount", SortDirection::Desc);
        let result = store.get_collection(ORDERS, &query).unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].get("Amount"), Some(json!(200)));
        assert_eq!(result[1].get("Amount"), Some(json!(120)));
        assert_eq!(result[2].get("Amount"), Some(json!(50)));
    }

    #[test]
    fn get_collection_applies_skip_and_top() {
        let (_dir, store) = make_store();
        for n in 0..3 {
            store
                .create_entity(
                    ORDERS,
                    &json!({ "ID": to_uuid(n + 1), "Amount": (n as i64 + 1) * 10 }),
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
                &json!({ "ID": to_uuid(1), "IsActiveEntity": true, "Status": "Released" }),
            )
            .unwrap();

        let active_key = EntityKey::new_active(to_uuid(1));
        let draft = store.draft_edit(ORDERS, &active_key).unwrap();
        assert_eq!(draft["IsActiveEntity"], false);

        let draft_key = active_key.sibling();
        store
            .patch_entity(ORDERS, &draft_key, &json!({ "Status": "InProgress" }))
            .unwrap();

        let activated = store.draft_activate(ORDERS, &draft_key).unwrap();
        assert_eq!(activated["Status"], "InProgress");
        assert_eq!(activated["IsActiveEntity"], true);
        assert_eq!(store.count_records(ORDERS, false), 0);
    }

    #[test]
    fn read_sibling_entity_returns_active() -> anyhow::Result<()> {
        let (_dir, store) = make_store();
        setup_data(&store)?;
        let key = EntityKey::new_active(to_uuid(1));
        store.draft_edit(ORDERS, &key)?;

        let sibling = store.read_sibling_entity(ORDERS, &key.sibling())?;
        assert!(sibling.is_active_entity());
        assert_eq!(sibling.id(), to_uuid(1));
        Ok(())
    }

    #[test]
    fn persists_across_reopen() {
        let dir = TempDir::new().unwrap();
        {
            let store = SqliteStore::open(dir.path()).unwrap();
            store
                .create_entity(
                    ORDERS,
                    &json!({ "ID": to_uuid(1), "IsActiveEntity": true, "Amount": 42 }),
                )
                .unwrap();
            store.commit();
        }

        let reopened = SqliteStore::open(dir.path()).unwrap();
        let key = EntityKey::new_active(to_uuid(1));
        let read = reopened
            .read_entity(ORDERS, &key, &ODataQuery::new())
            .unwrap();
        assert_eq!(read["Amount"], 42);
    }

    #[test]
    fn expand_draft_metadata() -> anyhow::Result<()> {
        let (_dir, store) = make_store();
        setup_data(&store)?;
        let key = EntityKey::new_active(to_uuid(1));
        store.draft_edit(ORDERS, &key)?;

        let query = ODataQuery::new().expand("DraftAdministrativeData");
        let r = store.read_entity(ORDERS, &key.sibling(), &query)?;
        assert!(r.draft_admin_data().is_some());
        Ok(())
    }

    #[test]
    fn expand() -> anyhow::Result<()> {
        let (_dir, store) = make_store();
        setup_data(&store)?;

        let q = ODataQuery::new()
            .expand("_Items")
            .expand("Customer")
            .order_by("Amount", SortDirection::Asc);
        let result = store.get_collection(ORDERS, &q)?;
        match result.as_slice() {
            [r, _] => {
                assert_eq!(r.get("_Items").unwrap().as_array().map(|a| a.len()), Some(2));
                assert_eq!(r.get("Customer").unwrap()["Name"], json!("Müller GmbH"));
            }
            x => panic!("selektion hat nicht geklappt: {:#?}", x),
        }

        let key = EntityKey::new_active(to_uuid(1));
        let r = store.read_entity(ORDERS, &key, &q)?;
        assert_eq!(r.get("_Items").unwrap().as_array().map(|a| a.len()), Some(2));
        assert_eq!(r.get("Customer").unwrap()["Name"], json!("Müller GmbH"));
        Ok(())
    }

    #[test]
    fn in_memory_store_works() {
        let store = SqliteStore::open_in_memory().unwrap();
        store
            .create_entity(ORDERS, &json!({ "ID": to_uuid(1), "Amount": 7 }))
            .unwrap();
        assert_eq!(store.count_records(ORDERS, false), 1);
    }
}
