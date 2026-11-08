    use super::*;
    use odata_yaml_store::YamlStore;
    use serde_json::json;
    use spec::FieldSpec;
    use tempfile::TempDir;

    const PACKAGE_ID: &str = "00000000-0000-0000-0000-0000000000a1";
    const ORDER_ID: &str = "00000000-0000-0000-0000-000000000001";
    const ITEM_ID: &str = "00000000-0000-0000-0000-000000000002";

    async fn store_with_metadata() -> (TempDir, YamlStore) {
        let dir = TempDir::new().unwrap();
        let store = YamlStore::open(dir.path()).unwrap();
        store
            .create_entity(
                "PackageSet",
                &json!({ "ID": PACKAGE_ID, "IsActiveEntity": true, "name": "sales" }),
            )
            .await
            .unwrap();
        store
            .create_entity(
                "EntitySet",
                &json!({ "ID": ORDER_ID, "IsActiveEntity": true,
                         "name": "Order", "plural": "Orders", "package_ID": PACKAGE_ID }),
            )
            .await
            .unwrap();
        store
            .create_entity(
                "EntitySet",
                &json!({ "ID": ITEM_ID, "IsActiveEntity": true,
                         "name": "Item", "plural": "Items", "package_ID": PACKAGE_ID }),
            )
            .await
            .unwrap();
        store
            .create_entity(
                "FieldSet",
                &json!({ "IsActiveEntity": true, "entity_ID": ORDER_ID,
                         "name": "status", "label": "Status", "type": "Edm.String",
                         "max_length": 20, "immutable": true }),
            )
            .await
            .unwrap();
        (dir, store)
    }

    async fn loaded(store: &YamlStore) -> DataDictionary {
        let mut ddic = DataDictionary::new();
        load_ddic_from_store(&mut ddic, store).await;
        ddic.activate();
        ddic
    }

    #[tokio::test]
    async fn loads_packages_entities_and_fields() {
        let (_dir, store) = store_with_metadata().await;
        let ddic = loaded(&store).await;

        let order = ddic.active_entity_by_setname("OrderSet").unwrap();
        assert_eq!(order.name(), "Order");
        let status = order.fields().find(|f| f.name() == "status").unwrap();
        assert!(matches!(
            status,
            FieldSpec::Atom {
                max_length: Some(20),
                immutable: true,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn derives_lookups_from_a_stored_relation() {
        let (_dir, store) = store_with_metadata().await;
        store
            .create_entity(
                "RelationSet",
                &json!({ "IsActiveEntity": true, "name": "order_items",
                         "one_ID": ORDER_ID, "source_field": "_Items",
                         "many_ID": ITEM_ID, "target_field": "order" }),
            )
            .await
            .unwrap();
        let ddic = loaded(&store).await;

        let order = ddic.active_entity_by_setname("OrderSet").unwrap();
        let items = order.fields().find(|f| f.name() == "_Items").unwrap();
        assert!(matches!(
            items,
            FieldSpec::Lookup {
                entity,
                collection: true,
                ..
            } if entity == "Item"
        ));
        assert_eq!(
            ddic.foreign_key_field(order, items).as_deref(),
            Some("order")
        );

        let navigation = navigations_from_ddic(&ddic)
            .into_iter()
            .find(|n| n.from_field.as_deref() == Some("_Items"))
            .unwrap();
        assert_eq!(navigation.from_set.as_deref(), Some("OrderSet"));
        assert_eq!(navigation.to_set.as_deref(), Some("ItemSet"));
        assert_eq!(navigation.to_field.as_deref(), Some("order_ID"));
        assert!(!navigation.single);
    }

    #[tokio::test]
    async fn resolves_relation_sides_by_entity_name() {
        let (_dir, store) = store_with_metadata().await;
        store
            .create_entity(
                "RelationSet",
                &json!({ "IsActiveEntity": true, "name": "by_name",
                         "source": "Order", "source_field": "_Positions",
                         "target": "Item", "target_field": "head" }),
            )
            .await
            .unwrap();
        store
            .create_entity(
                "RelationSet",
                &json!({ "IsActiveEntity": true, "name": "junk",
                         "source": "S", "target": "T" }),
            )
            .await
            .unwrap();
        let ddic = loaded(&store).await;

        assert!(ddic.relationships().any(|r| r.name() == "by_name"));
        assert!(!ddic.relationships().any(|r| r.name() == "junk"));
    }

    #[tokio::test]
    async fn loads_apps_and_skips_unknown_main_entity() {
        let (_dir, store) = store_with_metadata().await;
        store
            .create_entity(
                "AppSet",
                &json!({ "IsActiveEntity": true, "name": "orders", "title": "Orders",
                         "subtitle": "Sales orders", "icon": "sap-icon://cart",
                         "main_entity_ID": ORDER_ID, "package_ID": PACKAGE_ID }),
            )
            .await
            .unwrap();
        store
            .create_entity(
                "AppSet",
                &json!({ "IsActiveEntity": true, "name": "broken", "main_entity": "Nope" }),
            )
            .await
            .unwrap();
        let ddic = loaded(&store).await;

        let app = ddic.apps().find(|a| a.name() == "orders").unwrap();
        assert_eq!(app.main_entity(&ddic).name(), "Order");
        assert_eq!(app.subtitle().map(String::as_str), Some("Sales orders"));
        assert_eq!(app.icon().map(String::as_str), Some("sap-icon://cart"));
        assert!(!ddic.apps().any(|a| a.name() == "broken"));
    }
