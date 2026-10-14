//! Backend-uebergreifende Verhaltenssuite.
//!
//! Jedes Backend implementiert [`Harness`] und ruft [`conformance_tests!`] auf.
//! Damit ist "gleiches Verhalten" ueberpruefbar statt behauptet: Dokumenten-
//! und relationale Stores laufen gegen identische Erwartungen.
//!
//! Nur mit Feature `conformance` verfuegbar (in `[dev-dependencies]` setzen).

use odata::query::{BinOp, Expr, ODataQuery, SortDirection};
use serde_json::json;
use uuid::Uuid;

use crate::schema::{
    Association, Cardinality, Column, DataType, EntitySet, ForeignKeySide, Ownership, Schema,
};
use crate::{DataStore, DraftDataStore, EntityKey};

pub const ORDERS: &str = "OrderSet";
pub const ITEMS: &str = "ItemSet";
pub const CUSTOMERS: &str = "CustomerSet";

/// Stabile Test-UUIDs.
pub fn uuid(n: usize) -> Uuid {
    Uuid::parse_str(&format!("00000000-0000-0000-0000-{:012X}", n)).expect("uuid")
}

pub fn active(n: usize) -> EntityKey {
    EntityKey::new(uuid(n), true)
}

pub fn draft(n: usize) -> EntityKey {
    EntityKey::new(uuid(n), false)
}

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

/// Anbindung eines Backends an die Suite.
#[async_trait::async_trait]
pub trait Harness: Send + Sync {
    type Store: DraftDataStore;

    /// Frischer, leerer Store mit [`schema()`] als Metadaten.
    async fn create() -> Self;

    async fn store(&self) -> &Self::Store;

    /// Anzahl physisch gespeicherter Saetze je Aktivierungszustand.
    async fn count_state(&self, set_name: &str, is_active: bool) -> usize;

    /// Store schliessen und erneut oeffnen. `None` = fluechtiges Backend,
    /// der Persistenztest wird dann uebersprungen.
    async fn reopen(&self) -> Option<Self::Store> {
        None
    }
}

fn q() -> ODataQuery {
    ODataQuery::new()
}

fn bin(op: BinOp, left: Expr, right: Expr) -> Expr {
    Expr::BinaryOp {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

/// Grunddaten: 2 Kunden, 2 Auftraege, 3 Positionen - alles aktiv.
pub async fn seed<H: Harness>(h: &H) {
    let s = h.store().await;
    s.create_entity(
        CUSTOMERS,
        &json!({"ID": uuid(100), "IsActiveEntity": true, "Name": "Mueller GmbH"}),
    ).await
    .expect("customer 100");
    s.create_entity(
        CUSTOMERS,
        &json!({"ID": uuid(101), "IsActiveEntity": true, "Name": "Schmidt AG"}),
    ).await
    .expect("customer 101");
    s.create_entity(
        ORDERS,
        &json!({"ID": uuid(1), "IsActiveEntity": true, "Customer_ID": uuid(100), "Amount": 150, "Status": "New"}),
    ).await
    .expect("order 1");
    s.create_entity(
        ORDERS,
        &json!({"ID": uuid(2), "IsActiveEntity": true, "Customer_ID": uuid(101), "Amount": 480, "Status": "Released"}),
    ).await
    .expect("order 2");
    for (id, order, material, qty) in [
        (10, 1, "M-100", 5),
        (11, 1, "M-200", 2),
        (12, 2, "M-200", 2),
    ] {
        s.create_entity(
            ITEMS,
            &json!({"ID": uuid(id), "IsActiveEntity": true, "Order_ID": uuid(order),
                    "Material": material, "Qty": qty}),
        ).await
        .unwrap_or_else(|e| panic!("item {id}: {e}"));
    }
}

pub async fn assert_counts<H: Harness>(h: &H, set: &str, expected_active: usize, expected_draft: usize) {
    assert_eq!(
        h.count_state(set, true).await,
        expected_active,
        "{set}: aktive Saetze"
    );
    assert_eq!(
        h.count_state(set, false).await,
        expected_draft,
        "{set}: Draft-Saetze"
    );
}

// ---------------------------------------------------------------------------
// CRUD
// ---------------------------------------------------------------------------

pub async fn create_defaults_to_draft<H: Harness>() {
    let h = H::create().await;
    let rec = h
        .store().await
        .create_entity(ORDERS, &json!({"ID": uuid(1), "Status": "New"})).await
        .expect("create");
    assert!(
        !rec.is_active_entity(),
        "ohne IsActiveEntity entsteht ein Draft"
    );
    assert_counts(&h, ORDERS, 0, 1).await;
    let read = h.store().await.read_entity(ORDERS, &draft(1), &q()).await.expect("read");
    assert_eq!(read["Status"], json!("New"));
}

pub async fn create_active_bypasses_draft_flow<H: Harness>() {
    let h = H::create().await;
    let rec = h
        .store().await
        .create_entity(
            ORDERS,
            &json!({"ID": uuid(1), "IsActiveEntity": true, "Status": "New"}),
        ).await
        .expect("create");
    assert!(rec.is_active_entity());
    assert_counts(&h, ORDERS, 1, 0).await;
}

pub async fn patch_merges_only_given_fields<H: Harness>() {
    let h = H::create().await;
    h.store().await
        .create_entity(
            ORDERS,
            &json!({"ID": uuid(1), "Status": "New", "Amount": 100}),
        ).await
        .unwrap();
    h.store().await
        .patch_entity(ORDERS, &draft(1), &json!({"Status": "Released"})).await
        .expect("patch");
    let read = h.store().await.read_entity(ORDERS, &draft(1), &q()).await.unwrap();
    assert_eq!(read["Status"], json!("Released"));
    assert_eq!(read["Amount"], json!(100), "nicht gepatchte Felder bleiben");
}

pub async fn read_missing_entity_is_an_error<H: Harness>() {
    let h = H::create().await;
    assert!(
        h.store().await.read_entity(ORDERS, &active(999), &q()).await.is_err(),
        "fehlender Satz muss Err liefern, nicht panicken"
    );
}

pub async fn get_property_returns_single_value<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    let v = h
        .store().await
        .get_property(ORDERS, &active(1), "Status").await
        .expect("property");
    assert_eq!(v, json!("New"));
    assert!(
        h.store().await.get_property(ORDERS, &active(1), "Nope").await.is_err(),
        "unbekannte Property muss Err liefern"
    );
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

pub async fn orderby_sorts_ascending_and_descending<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    let asc = h
        .store().await
        .get_collection(ORDERS, &q().order_by("Amount", SortDirection::Asc)).await
        .unwrap();
    assert_eq!(
        asc.iter().map(|r| r.get("Amount")).collect::<Vec<_>>(),
        vec![Some(json!(150)), Some(json!(480))]
    );
    let desc = h
        .store().await
        .get_collection(ORDERS, &q().order_by("Amount", SortDirection::Desc)).await
        .unwrap();
    assert_eq!(
        desc.iter().map(|r| r.get("Amount")).collect::<Vec<_>>(),
        vec![Some(json!(480)), Some(json!(150))]
    );
}

pub async fn filter_by_equality<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    let query = q().filter(bin(
        BinOp::Eq,
        Expr::Identifier("Status".into()),
        Expr::String("Released".into()),
    ));
    let result = h.store().await.get_collection(ORDERS, &query).await.unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id(), uuid(2));
}

pub async fn filter_by_comparison<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    let query = q().filter(bin(
        BinOp::Gt,
        Expr::Identifier("Amount".into()),
        Expr::Number(200.0),
    ));
    let result = h.store().await.get_collection(ORDERS, &query).await.unwrap();
    assert_eq!(result.len(), 1, "nur Amount 480 ist groesser als 200");
    assert_eq!(result[0].id(), uuid(2));
}

pub async fn filter_combines_with_and<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    let query = q().filter(bin(
        BinOp::And,
        bin(
            BinOp::Gt,
            Expr::Identifier("Amount".into()),
            Expr::Number(100.0),
        ),
        bin(
            BinOp::Eq,
            Expr::Identifier("Status".into()),
            Expr::String("New".into()),
        ),
    ));
    let result = h.store().await.get_collection(ORDERS, &query).await.unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id(), uuid(1));
}

pub async fn skip_and_top_paginate<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    let mut query = q().order_by("Amount", SortDirection::Asc);
    query.skip = Some(1);
    query.top = Some(1);
    let result = h.store().await.get_collection(ORDERS, &query).await.unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].get("Amount"), Some(json!(480)));
}

pub async fn count_ignores_pagination<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    let mut query = q();
    query.skip = Some(1);
    query.top = Some(1);
    assert_eq!(h.store().await.count(ORDERS, &query).await, 2);
}

pub async fn drafts_shadow_active_records_in_collections<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    h.store().await.draft_edit(ORDERS, &active(1)).await.expect("draft_edit");
    h.store().await
        .patch_entity(ORDERS, &draft(1), &json!({"Status": "InProgress"})).await
        .unwrap();

    let all = h.store().await.get_collection(ORDERS, &q()).await.unwrap();
    assert_eq!(all.len(), 2, "der Draft verdeckt den aktiven Satz");
    let shadowed = all.iter().find(|r| r.id() == uuid(1)).expect("order 1");
    assert!(!shadowed.is_active_entity());
    assert_eq!(shadowed.get("Status"), Some(json!("InProgress")));
}

// ---------------------------------------------------------------------------
// $expand
// ---------------------------------------------------------------------------

pub async fn expand_to_many_returns_children<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    let r = h
        .store().await
        .read_entity(ORDERS, &active(1), &q().expand("_Items")).await
        .expect("read");
    let items = r.get("_Items").expect("_Items");
    assert_eq!(items.as_array().map(|a| a.len()), Some(2));
}

pub async fn expand_to_one_returns_parent<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    let r = h
        .store().await
        .read_entity(ORDERS, &active(1), &q().expand("Customer")).await
        .expect("read");
    assert_eq!(r.get("Customer").expect("Customer")["Name"], json!("Mueller GmbH"));
}

pub async fn expand_to_one_survives_select<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    // $select nennt den Fremdschluessel Customer_ID nicht; die ToOne-Expansion
    // muss ihn trotzdem lesen koennen. (Bewusst keine Komposition mit
    // Gegenrichtung wie Item.Order: die Dokumenten-Stores fuehren sie unter
    // dem FK-Namen, nicht unter der Navigation.)
    let rows = h
        .store().await
        .get_collection(ORDERS, &q().select("Amount").expand("Customer")).await
        .expect("collection");
    assert_eq!(rows.len(), 2);
    for row in &rows {
        let customer = row.get("Customer").expect("Customer");
        assert!(!customer.is_null(), "Customer ist null trotz gesetztem Fremdschluessel");
        assert!(customer["Name"].is_string());
    }
}

pub async fn expand_in_collection<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    let rows = h
        .store().await
        .get_collection(
            ORDERS,
            &q().expand("_Items")
                .expand("Customer")
                .order_by("Amount", SortDirection::Asc),
        ).await
        .expect("collection");
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].get("_Items").and_then(|v| v.as_array().map(|a| a.len())),
        Some(2)
    );
    assert_eq!(rows[0].get("Customer").unwrap()["Name"], json!("Mueller GmbH"));
}

pub async fn expand_unknown_navigation_is_an_error<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    assert!(
        h.store().await
            .read_entity(ORDERS, &active(1), &q().expand("_Nope")).await
            .is_err(),
        "unbekannte Navigation muss Err liefern"
    );
}

pub async fn expand_draft_administrative_data<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    h.store().await.draft_edit(ORDERS, &active(1)).await.unwrap();
    let r = h
        .store().await
        .read_entity(ORDERS, &draft(1), &q().expand("DraftAdministrativeData")).await
        .expect("read draft");
    assert!(r.draft_admin_data().is_some());
}

// ---------------------------------------------------------------------------
// Draft-Flow
// ---------------------------------------------------------------------------

pub async fn draft_edit_activate_roundtrip<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    let d = h.store().await.draft_edit(ORDERS, &active(1)).await.expect("edit");
    assert_eq!(d["IsActiveEntity"], json!(false));

    h.store().await
        .patch_entity(ORDERS, &draft(1), &json!({"Status": "InProgress"})).await
        .unwrap();
    let activated = h.store().await.draft_activate(ORDERS, &draft(1)).await.expect("activate");
    assert_eq!(activated["Status"], json!("InProgress"));
    assert_eq!(activated["IsActiveEntity"], json!(true));
    assert_counts(&h, ORDERS, 2, 0).await;
}

pub async fn draft_edit_cascades_to_children<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    h.store().await.draft_edit(ORDERS, &active(1)).await.unwrap();
    assert_counts(&h, ORDERS, 2, 1).await;
    assert_counts(&h, ITEMS, 3, 2).await;
}

pub async fn draft_edit_rejects_draft_key<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    assert!(h.store().await.draft_edit(ORDERS, &draft(1)).await.is_err());
}

pub async fn draft_edit_child_without_parent_draft_fails<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    assert!(
        h.store().await.draft_edit(ITEMS, &active(10)).await.is_err(),
        "ein Kind allein darf nicht in den Draft"
    );
}

pub async fn draft_activate_rejects_active_key<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    assert!(h.store().await.draft_activate(ORDERS, &active(1)).await.is_err());
}

pub async fn draft_activate_cascades_to_children<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    h.store().await.draft_edit(ORDERS, &active(2)).await.unwrap();
    h.store().await
        .create_entity(
            ITEMS,
            &json!({"ID": uuid(13), "IsActiveEntity": false, "Order_ID": uuid(2),
                    "Material": "M-300", "Qty": 42}),
        ).await
        .expect("neues Kind im Draft");
    assert_counts(&h, ITEMS, 3, 2).await;

    h.store().await.draft_activate(ORDERS, &draft(2)).await.unwrap();
    assert_counts(&h, ORDERS, 2, 0).await;
    assert_counts(&h, ITEMS, 4, 0).await;

    let r = h
        .store().await
        .read_entity(ORDERS, &active(2), &q().expand("_Items")).await
        .unwrap();
    assert_eq!(r.get("_Items").unwrap().as_array().map(|a| a.len()), Some(2));
}

pub async fn draft_prepare_returns_the_draft<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    h.store().await.draft_edit(ORDERS, &active(1)).await.unwrap();
    let p = h.store().await.draft_prepare(ORDERS, &draft(1)).await.expect("prepare");
    assert_eq!(p["ID"], json!(uuid(1)));
    assert_eq!(p["IsActiveEntity"], json!(false));
}

pub async fn read_sibling_entity_crosses_activation_state<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    h.store().await.draft_edit(ORDERS, &active(1)).await.unwrap();
    let sibling = h
        .store().await
        .read_sibling_entity(ORDERS, &draft(1)).await
        .expect("sibling");
    assert!(sibling.is_active_entity());
    assert_eq!(sibling.id(), uuid(1));
}

// ---------------------------------------------------------------------------
// Loeschen / Kaskaden
// ---------------------------------------------------------------------------

pub async fn delete_active_parent_cascades<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    h.store().await.delete_entity(ORDERS, &active(2)).await.expect("delete");
    assert_counts(&h, ORDERS, 1, 0).await;
    assert_counts(&h, ITEMS, 2, 0).await;
    assert_counts(&h, CUSTOMERS, 2, 0).await;
}

pub async fn delete_child_leaves_parent<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    h.store().await.delete_entity(ITEMS, &active(12)).await.expect("delete");
    assert_counts(&h, ORDERS, 2, 0).await;
    assert_counts(&h, ITEMS, 2, 0).await;
}

pub async fn delete_draft_removes_both_trees<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    h.store().await.draft_edit(ORDERS, &active(2)).await.unwrap();
    h.store().await
        .create_entity(
            ITEMS,
            &json!({"ID": uuid(13), "IsActiveEntity": false, "Order_ID": uuid(2),
                    "Material": "M-300", "Qty": 1}),
        ).await
        .unwrap();
    h.store().await.delete_entity(ORDERS, &draft(2)).await.expect("delete draft");
    assert_counts(&h, ORDERS, 1, 0).await;
    assert_counts(&h, ITEMS, 2, 0).await;
}

pub async fn delete_entity_tree_only_keeps_sibling<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    h.store().await.draft_edit(ORDERS, &active(2)).await.unwrap();
    assert_counts(&h, ITEMS, 3, 1).await;
    h.store().await
        .delete_entity_tree_only(ITEMS, &draft(12)).await
        .expect("tree only");
    assert_counts(&h, ITEMS, 3, 0).await;
}

/// Regression: beim Aktivieren darf ein nur aktiv vorhandenes Kind nicht
/// angefasst werden (Draft-Ueberdeckung bei der Kindsuche).
pub async fn draft_activate_keeps_active_only_child<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    h.store().await.draft_edit(ORDERS, &active(2)).await.unwrap();
    h.store().await
        .create_entity(
            ITEMS,
            &json!({"ID": uuid(20), "IsActiveEntity": true, "Order_ID": uuid(2),
                    "Material": "M-900", "Qty": 1}),
        ).await
        .expect("aktives Kind ohne Draft-Zwilling");
    assert_counts(&h, ITEMS, 4, 1).await;

    h.store().await.draft_activate(ORDERS, &draft(2)).await.unwrap();
    assert_counts(&h, ITEMS, 4, 0).await;
    let survivor = h.store().await.read_entity(ITEMS, &active(20), &q()).await.expect("Kind 20");
    assert_eq!(survivor["Material"], json!("M-900"));
}

/// Regression: das Loeschen eines aktiven Baums muss den aktiven Kindsatz
/// treffen, nicht den Draft, der ihn in Collections verdeckt.
pub async fn delete_active_parent_removes_active_children<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    h.store().await.draft_edit(ORDERS, &active(2)).await.unwrap();
    assert_counts(&h, ITEMS, 3, 1).await;
    h.store().await.delete_entity(ORDERS, &active(2)).await.unwrap();
    assert_counts(&h, ITEMS, 2, 1).await;
    assert!(
        h.store().await.read_entity(ITEMS, &active(12), &q()).await.is_err(),
        "aktives Kind darf nicht als Waise zurueckbleiben"
    );
}

// ---------------------------------------------------------------------------
// Persistenz
// ---------------------------------------------------------------------------

pub async fn data_survives_reopen<H: Harness>() {
    let h = H::create().await;
    seed(&h).await;
    let Some(reopened) = h.reopen().await else {
        return; // fluechtiges Backend
    };
    let read = reopened
        .read_entity(ORDERS, &active(1), &ODataQuery::new()).await
        .expect("nach reopen lesbar");
    assert_eq!(read["Amount"], json!(150));
}

/// Erzeugt fuer jede Konformitaetsfunktion einen eigenen `#[tokio::test]`.
#[macro_export]
macro_rules! conformance_tests {
    ($harness:ty) => {
        $crate::conformance_tests!(
            $harness =>
            create_defaults_to_draft,
            create_active_bypasses_draft_flow,
            patch_merges_only_given_fields,
            read_missing_entity_is_an_error,
            get_property_returns_single_value,
            orderby_sorts_ascending_and_descending,
            filter_by_equality,
            filter_by_comparison,
            filter_combines_with_and,
            skip_and_top_paginate,
            count_ignores_pagination,
            drafts_shadow_active_records_in_collections,
            expand_to_many_returns_children,
            expand_to_one_returns_parent,
            expand_in_collection,
            expand_to_one_survives_select,
            expand_unknown_navigation_is_an_error,
            expand_draft_administrative_data,
            draft_edit_activate_roundtrip,
            draft_edit_cascades_to_children,
            draft_edit_rejects_draft_key,
            draft_edit_child_without_parent_draft_fails,
            draft_activate_rejects_active_key,
            draft_activate_cascades_to_children,
            draft_prepare_returns_the_draft,
            read_sibling_entity_crosses_activation_state,
            delete_active_parent_cascades,
            delete_child_leaves_parent,
            delete_draft_removes_both_trees,
            delete_entity_tree_only_keeps_sibling,
            draft_activate_keeps_active_only_child,
            delete_active_parent_removes_active_children,
            data_survives_reopen
        );
    };
    ($harness:ty => $($name:ident),+ $(,)?) => {
        $(
            #[tokio::test]
            async fn $name() {
                $crate::conformance::$name::<$harness>().await;
            }
        )+
    };
}
