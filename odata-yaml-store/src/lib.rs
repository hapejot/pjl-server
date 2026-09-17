//! YAML-Implementierung des in `data-store` definierten Interfaces.
//!
//! Jeder Datensatz liegt als eigene Datei unter
//! `<root>/<EntitySet>/{active,draft}/<uuid>.yaml`.

// Re-export der abstrakten Schnittstelle, damit Nutzer nur ein Crate brauchen.
pub use data_store::{
    DataRecord, DataStore, DraftAction, DraftAdministrativeData, DraftDataStore, DraftMessage,
    EntityKey, Navigation, StoreError,
};

pub mod yaml_store;
pub use yaml_store::YamlStore;

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

    fn make_store() -> (TempDir, impl DraftDataStore) {
        let dir = TempDir::new().unwrap();
        let store = YamlStore::open(dir.path()).unwrap();
        (dir, store)
    }

    fn to_uuid(n: usize) -> Uuid {
        Uuid::parse_str(&format!("00000000-0000-0000-0000-{:012X}", n)).expect("UUID mismatch")
    }

    async fn setup_data(store: &mut impl DraftDataStore) -> anyhow::Result<()> {
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
        ).await?;
        // store.draft_activate(CUSTOMERS, &EntityKey::new(cust_mueller, false)).await?;

        store.create_entity(
            CUSTOMERS,
            &json!({"ID": cust_schmidt, "IsActiveEntity":true, "Name": "Schmidt AG"}),
        ).await?;
        // store.draft_activate(CUSTOMERS, &EntityKey::new(cust_schmidt, false)).await?;

        store.create_entity(
        ORDERS,
        &json!({ "ID": to_uuid(1), "IsActiveEntity":true, "Customer_ID": cust_mueller, "Amount": 150, "Status": "New" }),
    ).await?;
        store.create_entity(
        ORDERS,
        &json!({ "ID": to_uuid(2), "IsActiveEntity":true, "Customer_ID": cust_schmidt, "Amount": 480, "Status": "Released" }),
    ).await?;
        store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(10), "IsActiveEntity":true, "Order_ID": to_uuid(1), "Material": "M-100", "Qty": 5 }),
        ).await?;
        store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(11), "IsActiveEntity":true, "Order_ID": to_uuid(1), "Material": "M-200", "Qty": 2 }),
        ).await?;
        let _ = store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(12), "IsActiveEntity":true, "Order_ID": to_uuid(2), "Material": "M-200", "Qty": 2 }),
        ).await;
        Ok(())
    }
    #[tokio::test]
    async fn create() {
        // Entites sind automatisch im Draft Modus, wenn sie angelegt werden.
        // Das bedeutet, dass `IsActiveEntity` beim Erstellen auf `false` gesetzt wird.
        // Nur so können sie dann auch wieder gelesen werden.

        let id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let (_dir, store) = make_store();
        let data = json!({ "ID": id, "CustomerName": "Mueller GmbH", "Amount": 150 });
        store.create_entity(ORDERS, &data).await.unwrap();

        let key = EntityKey::new(id, false);
        let q = ODataQuery::new();
        let read = store.read_entity(ORDERS, &key, &q).await.unwrap();
        assert_eq!(read["CustomerName"], "Mueller GmbH");
    }

    #[tokio::test]
    async fn initial_data() -> anyhow::Result<()> {
        let (dir, mut store) = make_store();
        setup_data(&mut store).await?;
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

    #[tokio::test]
    async fn edit_full_cycle() -> anyhow::Result<()> {
        let (dir, mut store) = make_store();
        setup_data(&mut store).await.unwrap();
        let key = &EntityKey {
            id: to_uuid(2),
            is_active_entity: true,
        };
        store.draft_edit(ORDERS, key).await?;
        let rec = store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(13), "IsActiveEntity":false, "Order_ID": to_uuid(2), "Material": "M-300", "Qty": 42}),
        ).await?;
        assert!(!rec.is_active_entity());
        let rec = store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(14), "IsActiveEntity":false, "Order_ID": to_uuid(2), "Material": "M-400", "Qty": 4}),
        ).await?;
        assert!(!rec.is_active_entity());
        check_entity_dirs(&dir, ORDERS, 2, 1)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 3, 3)?;
        store.draft_activate(ORDERS, &key.sibling()).await?;
        check_entity_dirs(&dir, ORDERS, 2, 0)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 5, 0)?;

        let query = ODataQuery::new().expand("_Items");
        let r = store.read_entity(ORDERS, &key, &query).await?;
        let items = r.get("_Items").unwrap();
        assert_eq!(items.as_array().unwrap().len(), 3);

        // versuche einen Item alleine zu editieren. Sollte nicht gehen
        //
        let item_key = EntityKey {
            id: to_uuid(13),
            is_active_entity: true,
        };
        let r = store.draft_edit(ORDER_ITEMS, &item_key).await;
        assert!(r.is_err());

        Ok(())
    }

    #[tokio::test]
    async fn delete_parent() -> anyhow::Result<()> {
        let (dir, mut store) = make_store();
        setup_data(&mut store).await.unwrap();
        let key = &EntityKey {
            id: to_uuid(2),
            is_active_entity: true,
        };
        store.delete_entity(ORDERS, key).await?;
        check_entity_dirs(&dir, ORDERS, 1, 0)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 2, 0)?;
        Ok(())
    }

    #[tokio::test]
    async fn delete_parent_draft() -> anyhow::Result<()> {
        let (dir, mut store) = make_store();
        setup_data(&mut store).await.unwrap();
        let key = &EntityKey {
            id: to_uuid(2),
            is_active_entity: true,
        };
        store.draft_edit(ORDERS, key).await?;
        let _rec = store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(13), "IsActiveEntity":false, "Order_ID": to_uuid(2), "Material": "M-300", "Qty": 42}),
        ).await?;
        check_entity_dirs(&dir, ORDERS, 2, 1)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 3, 2)?;
        store.delete_entity(ORDERS, &key.sibling()).await?;
        check_entity_dirs(&dir, ORDERS, 1, 0)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 2, 0)?;
        Ok(())
    }

    #[tokio::test]
    async fn delete_child() -> anyhow::Result<()> {
        let (dir, mut store) = make_store();
        setup_data(&mut store).await.unwrap();
        let key = &EntityKey {
            id: to_uuid(12),
            is_active_entity: true,
        };
        store.delete_entity(ORDER_ITEMS, key).await?;
        check_entity_dirs(&dir, ORDERS, 2, 0)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 2, 0)?;
        Ok(())
    }

    #[tokio::test]
    async fn delete_child_draft() -> anyhow::Result<()> {
        let (dir, mut store) = make_store();
        setup_data(&mut store).await.unwrap();
        let parent_key = EntityKey::new_active(to_uuid(2));
        let key = &EntityKey::new_active(to_uuid(12));
        store.draft_edit(ORDERS, &parent_key).await?;
        check_entity_dirs(&dir, ORDERS, 2, 1)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 3, 1)?;
        let _rec = store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(13), "IsActiveEntity":false, "Order_ID": to_uuid(2), "Material": "M-300", "Qty": 42}),
        ).await?;
        check_entity_dirs(&dir, ORDERS, 2, 1)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 3, 2)?;
        store.delete_entity(ORDER_ITEMS, &key.sibling()).await?;
        check_entity_dirs(&dir, ORDERS, 2, 1)?;
        check_entity_dirs(&dir, CUSTOMERS, 2, 0)?;
        check_entity_dirs(&dir, ORDER_ITEMS, 2, 1)?;
        Ok(())
    }

    /// Regression: beim Aktivieren eines Drafts darf ein Kind, das es *nur*
    /// aktiv gibt, nicht angefasst werden. Zuvor lieferte die Kindsuche wegen
    /// der Draft-Ueberdeckung den aktiven Satz zurueck, der dann im Zuge der
    /// "Aktivierung" geloescht wurde.
    #[tokio::test]
    async fn draft_activate_keeps_active_only_child() -> anyhow::Result<()> {
        let (dir, mut store) = make_store();
        setup_data(&mut store).await?;
        let parent = EntityKey::new_active(to_uuid(2));
        store.draft_edit(ORDERS, &parent).await?;

        // Kind direkt aktiv anlegen - es hat damit keinen Draft-Zwilling.
        store.create_entity(
            ORDER_ITEMS,
            &json!({ "ID": to_uuid(20), "IsActiveEntity": true, "Order_ID": to_uuid(2), "Material": "M-900", "Qty": 1}),
        ).await?;
        check_entity_dirs(&dir, ORDER_ITEMS, 4, 1)?;

        store.draft_activate(ORDERS, &parent.sibling()).await?;

        // 10, 11, 12 (aktiviert) und 20 (unveraendert aktiv)
        check_entity_dirs(&dir, ORDER_ITEMS, 4, 0)?;
        let survivor = store.read_entity(
            ORDER_ITEMS,
            &EntityKey::new_active(to_uuid(20)),
            &ODataQuery::new(),
        ).await?;
        assert_eq!(survivor["Material"], json!("M-900"));
        Ok(())
    }

    /// Regression: das Loeschen eines aktiven Baums muss den aktiven Kindsatz
    /// treffen - nicht den Draft, der ihn in Collections ueberdeckt.
    #[tokio::test]
    async fn delete_active_parent_removes_active_children() -> anyhow::Result<()> {
        let (dir, mut store) = make_store();
        setup_data(&mut store).await?;
        let parent = EntityKey::new_active(to_uuid(2));
        store.draft_edit(ORDERS, &parent).await?;
        check_entity_dirs(&dir, ORDER_ITEMS, 3, 1)?;

        store.delete_entity(ORDERS, &parent).await?;

        // Der aktive Kindsatz 12 ist mit dem aktiven Parent verschwunden und
        // bleibt nicht als Waise zurueck.
        check_entity_dirs(&dir, ORDER_ITEMS, 2, 1)?;
        assert!(
            store
                .read_entity(
                    ORDER_ITEMS,
                    &EntityKey::new_active(to_uuid(12)),
                    &ODataQuery::new()
                ).await
                .is_err()
        );
        Ok(())
    }

    /// `draft_activate` arbeitet nur auf Draft-Schluesseln.
    #[tokio::test]
    async fn draft_activate_rejects_active_key() -> anyhow::Result<()> {
        let (_dir, mut store) = make_store();
        setup_data(&mut store).await?;
        assert!(
            store
                .draft_activate(ORDERS, &EntityKey::new_active(to_uuid(2))).await
                .is_err()
        );
        Ok(())
    }

    #[tokio::test]
    async fn patch_entity_merges_fields() {
        let (_dir, store) = make_store();
        let id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let data = json!({ "ID": id, "Status": "New", "Amount": 100 });
        store.create_entity(ORDERS, &data).await.unwrap();

        let key = EntityKey::new(id, false);
        store
            .patch_entity(ORDERS, &key, &json!({ "Status": "Released" })).await
            .unwrap();

        let q = ODataQuery::new();
        let read = store.read_entity(ORDERS, &key, &q).await.unwrap();
        assert_eq!(read["Status"], "Released");
        assert_eq!(read["Amount"], 100); // unveraendert
    }

    #[tokio::test]
    async fn get_collection_with_filter_and_orderby() {
        let (_dir, store) = make_store();
        store
            .create_entity(
                ORDERS,
                &json!({ "ID": Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(), "Amount": 50, "Status": "New" }),
            ).await
            .unwrap();
        store
            .create_entity(
                ORDERS,
                &json!({ "ID": Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(), "Amount": 200, "Status": "Released" }),
            ).await
            .unwrap();
        store
            .create_entity(
                ORDERS,
                &json!({ "ID": Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap(), "Amount": 120, "Status": "Released" }),
            ).await
            .unwrap();

        let query = ODataQuery::new().order_by("Amount", SortDirection::Desc);

        let result = store.get_collection(ORDERS, &query).await.unwrap();
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

    #[tokio::test]
    async fn get_collection_applies_skip_and_top() {
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
                ).await
                .unwrap();
        }

        let mut query = ODataQuery::new().order_by("Amount", SortDirection::Asc);
        query.skip = Some(1);
        query.top = Some(1);

        let result = store.get_collection(ORDERS, &query).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].get("Amount"), Some(json!(20)));
        // $count ignoriert die Seitenaufteilung
        assert_eq!(store.count(ORDERS, &query).await, 3);
    }

    #[tokio::test]
    async fn draft_edit_activate_roundtrip() {
        let (_dir, store) = make_store();
        store
            .create_entity(
                ORDERS,
                &json!({ "ID": "00000000-0000-0000-0000-000000000001",
                         "IsActiveEntity": true,
                         "Status": "Released" }),
            ).await
            .unwrap();

        let active_key = EntityKey::new_active("00000000-0000-0000-0000-000000000001");

        let draft = store.draft_edit(ORDERS, &active_key).await.unwrap();
        assert_eq!(draft["IsActiveEntity"], false);

        // Draft bearbeiten
        let draft_key = active_key.sibling();
        store
            .patch_entity(ORDERS, &draft_key, &json!({ "Status": "InProgress" })).await
            .unwrap();

        // Aktivieren
        let activated = store.draft_activate(ORDERS, &draft_key).await.unwrap();
        assert_eq!(activated["Status"], "InProgress");
        assert_eq!(activated["IsActiveEntity"], true);

        // let debug: &dyn DataStoreDebugExt = &store;
        // let remaining_drafts: Vec<_> = debug
        // .get_records(ORDERS).await
        // .into_iter()
        // .filter(|r| r["IsActiveEntity"] == false)
        // .collect();
        // assert!(remaining_drafts.is_empty());
    }

    #[tokio::test]
    async fn persists_across_reopen() {
        let dir = TempDir::new().unwrap();

        {
            let store = YamlStore::open(dir.path()).unwrap();
            store
                .create_entity(
                    ORDERS,
                    &json!({ "ID": "00000000-0000-0000-0000-000000000001",
                                                    "IsActiveEntity": true,
                                                    "Amount": 42 }),
                ).await
                .unwrap();
        }

        let reopened = YamlStore::open(dir.path()).unwrap();
        let key = EntityKey::new_active("00000000-0000-0000-0000-000000000001");
        let q = ODataQuery::new();
        let read = reopened.read_entity(ORDERS, &key, &q).await.unwrap();
        assert_eq!(read["Amount"], 42);
    }
    #[tokio::test]
    async fn expand_draft_metadata() -> anyhow::Result<()> {
        let (_dir, mut store) = make_store();
        setup_data(&mut store).await.unwrap();
        let key = EntityKey::new_active(to_uuid(1));
        store.draft_edit(ORDERS, &key).await?;
        let query = ODataQuery::new().expand("DraftAdministrativeData");
        let r = store.read_entity(ORDERS, &key.sibling(), &query).await?;
        assert!(r.draft_admin_data().is_some());
        Ok(())
    }
    #[tokio::test]
    async fn expand() -> anyhow::Result<()> {
        let (_dir, mut store) = make_store();
        setup_data(&mut store).await?;

        let q = ODataQuery::new()
            .expand("_Items")
            .expand("Customer")
            .order_by("Amount", SortDirection::Asc);
        let result = store.get_collection(ORDERS, &q).await?;
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
        let r = store.read_entity(ORDERS, &key, &q).await?;
        assert_eq!(
            r.get("_Items").unwrap().as_array().map(|a| a.len()),
            Some(2)
        );
        assert_eq!(r.get("Customer").unwrap()["Name"], json!("Müller GmbH"));
        Ok(())
    }
}
