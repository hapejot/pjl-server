//! Eigenschaften, die es nur im relationalen Backend gibt: DDL, Migration,
//! Fremdschluessel-Kaskaden und SQL-Uebersetzung.

use std::sync::Arc;

use data_store::conformance::{self, active, draft, uuid, ITEMS, ORDERS};
use data_store::schema::{Column, DataType, EntitySet, Schema, StaticSchema};
use data_store::{DataStore, DraftDataStore, EntityKey};
use odata::query::{BinOp, Expr, ODataQuery, SortDirection};
use serde_json::json;
use sqlite_relational_store::RelationalStore;
use tempfile::TempDir;

fn store() -> RelationalStore {
    RelationalStore::open_in_memory(Arc::new(StaticSchema::new(conformance::schema())))
        .expect("store")
}

fn seeded() -> RelationalStore {
    let store = store();
    store
        .create_entity(
            "CustomerSet",
            &json!({"ID": uuid(100), "IsActiveEntity": true, "Name": "Mueller GmbH"}),
        )
        .unwrap();
    store
        .create_entity(
            ORDERS,
            &json!({"ID": uuid(1), "IsActiveEntity": true, "Customer_ID": uuid(100),
                    "Amount": 150, "Status": "New"}),
        )
        .unwrap();
    for (id, material) in [(10, "M-100"), (11, "M-200")] {
        store
            .create_entity(
                ITEMS,
                &json!({"ID": uuid(id), "IsActiveEntity": true, "Order_ID": uuid(1),
                        "Material": material, "Qty": 1}),
            )
            .unwrap();
    }
    store
}

fn bin(op: BinOp, left: Expr, right: Expr) -> Expr {
    Expr::BinaryOp {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

// ---------------------------------------------------------------------------
// Schema-Durchsetzung
// ---------------------------------------------------------------------------

#[test]
fn unknown_field_on_create_is_rejected() {
    let store = store();
    let err = store
        .create_entity(
            ORDERS,
            &json!({"ID": uuid(1), "IsActiveEntity": true, "Bogus": 1}),
        )
        .expect_err("unbekanntes Feld muss auffallen");
    assert!(err.to_string().contains("Bogus"), "{err}");
}

#[test]
fn unknown_field_on_patch_is_rejected() {
    let store = seeded();
    assert!(store
        .patch_entity(ORDERS, &active(1), &json!({"Bogus": 1}))
        .is_err());
}

#[test]
fn unknown_entity_set_is_rejected() {
    let store = store();
    assert!(store.get_collection("NichtDaSet", &ODataQuery::new()).is_err());
}

#[test]
fn type_mismatch_is_reported() {
    let store = store();
    let err = store
        .create_entity(
            ORDERS,
            &json!({"ID": uuid(1), "IsActiveEntity": true, "Amount": {"nested": true}}),
        )
        .expect_err("Objekt passt nicht in eine Int-Spalte");
    assert!(err.to_string().contains("Amount"), "{err}");
}

// ---------------------------------------------------------------------------
// SQL-Uebersetzung
// ---------------------------------------------------------------------------

#[test]
fn filter_on_unknown_column_is_an_error_not_an_empty_result() {
    let store = seeded();
    let query = ODataQuery::new().filter(bin(
        BinOp::Eq,
        Expr::Identifier("Nope".into()),
        Expr::String("x".into()),
    ));
    let err = store
        .get_collection(ORDERS, &query)
        .expect_err("Tippfehler darf nicht als 'kein Treffer' durchgehen");
    assert!(err.to_string().contains("Nope"), "{err}");
}

#[test]
fn untranslatable_function_fails_loudly() {
    let store = seeded();
    let query = ODataQuery::new().filter(Expr::FunctionCall {
        name: "floor".into(),
        args: vec![Expr::Identifier("Amount".into())],
    });
    let err = store
        .get_collection(ORDERS, &query)
        .expect_err("floor kennt SQLite nicht - lieber Fehler als falsches Ergebnis");
    assert!(err.to_string().contains("floor"), "{err}");
}

#[test]
fn path_expressions_in_filter_are_rejected() {
    let store = seeded();
    let query = ODataQuery::new().filter(bin(
        BinOp::Eq,
        Expr::Identifier("Customer/Name".into()),
        Expr::String("x".into()),
    ));
    assert!(store.get_collection(ORDERS, &query).is_err());
}

#[test]
fn string_functions_translate_to_sql() {
    let store = seeded();
    let cases = vec![
        ("contains", vec![Expr::Identifier("Status".into()), Expr::String("e".into())]),
        ("startswith", vec![Expr::Identifier("Status".into()), Expr::String("Ne".into())]),
        ("endswith", vec![Expr::Identifier("Status".into()), Expr::String("ew".into())]),
    ];
    for (name, args) in cases {
        let query = ODataQuery::new().filter(Expr::FunctionCall {
            name: name.into(),
            args,
        });
        let result = store.get_collection(ORDERS, &query).expect(name);
        assert_eq!(result.len(), 1, "{name} sollte 'New' treffen");
    }
}

/// Werte gehen als Parameter in die Anweisung - ein Anfuehrungszeichen im
/// Suchbegriff darf nichts kaputt machen.
#[test]
fn filter_values_are_bound_not_interpolated() {
    let store = seeded();
    let query = ODataQuery::new().filter(bin(
        BinOp::Eq,
        Expr::Identifier("Status".into()),
        Expr::String("'; DROP TABLE \"OrderSet\"; --".into()),
    ));
    assert_eq!(store.get_collection(ORDERS, &query).unwrap().len(), 0);
    // Tabelle steht noch.
    assert_eq!(store.get_collection(ORDERS, &ODataQuery::new()).unwrap().len(), 1);
}

#[test]
fn orderby_on_unknown_column_is_an_error() {
    let store = seeded();
    let query = ODataQuery::new().order_by("Nope", SortDirection::Asc);
    assert!(store.get_collection(ORDERS, &query).is_err());
}

#[test]
fn not_and_null_comparison_translate() {
    let store = seeded();
    let query = ODataQuery::new().filter(Expr::UnaryNot(Box::new(bin(
        BinOp::Eq,
        Expr::Identifier("Status".into()),
        Expr::String("New".into()),
    ))));
    assert_eq!(store.get_collection(ORDERS, &query).unwrap().len(), 0);

    let is_null = ODataQuery::new().filter(bin(
        BinOp::Eq,
        Expr::Identifier("Status".into()),
        Expr::Null,
    ));
    assert_eq!(store.get_collection(ORDERS, &is_null).unwrap().len(), 0);
}

// ---------------------------------------------------------------------------
// Fremdschluessel und Kaskaden
// ---------------------------------------------------------------------------

/// Beweist, dass die Kaskade die Datenbank macht: `delete_entity_tree_only`
/// setzt genau ein DELETE ab, die Kinder verschwinden trotzdem.
#[test]
fn composition_cascade_is_enforced_by_the_database() {
    let store = seeded();
    assert_eq!(store.count_state(ITEMS, true), 2);
    store
        .delete_entity_tree_only(ORDERS, &active(1))
        .expect("delete");
    assert_eq!(
        store.count_state(ITEMS, true),
        0,
        "ON DELETE CASCADE muss die Positionen mitnehmen"
    );
}

#[test]
fn child_under_missing_parent_is_rejected() {
    let store = store();
    let err = store
        .create_entity(
            ITEMS,
            &json!({"ID": uuid(10), "IsActiveEntity": true, "Order_ID": uuid(999),
                    "Material": "M-1", "Qty": 1}),
        )
        .expect_err("Position ohne Auftrag");
    assert!(format!("{err}").contains("999") || format!("{err}").contains("nicht gefunden"));
}

/// Der Fremdschluessel schliesst `IsActiveEntity` ein: ein Draft-Kind kann
/// nicht unter einem rein aktiven Parent haengen.
#[test]
fn draft_child_requires_a_draft_parent() {
    let store = seeded();
    let err = store
        .create_entity(
            ITEMS,
            &json!({"ID": uuid(13), "IsActiveEntity": false, "Order_ID": uuid(1),
                    "Material": "M-3", "Qty": 1}),
        )
        .expect_err("kein Draft-Parent vorhanden");
    assert!(format!("{err}").len() > 0);
}

// ---------------------------------------------------------------------------
// Abgeleitete Draft-Flags
// ---------------------------------------------------------------------------

#[test]
fn has_draft_entity_is_computed_from_the_sibling() {
    let store = seeded();
    let before = store.read_entity(ORDERS, &active(1), &ODataQuery::new()).unwrap();
    assert!(!before.has_draft_entity());

    store.draft_edit(ORDERS, &active(1)).unwrap();

    let after = store.read_entity(ORDERS, &active(1), &ODataQuery::new()).unwrap();
    assert!(
        after.has_draft_entity(),
        "abgeleitet aus der Existenz des Drafts, nicht gespeichert"
    );
    let draft_row = store.read_entity(ORDERS, &draft(1), &ODataQuery::new()).unwrap();
    assert!(draft_row.has_active_entity());
}

// ---------------------------------------------------------------------------
// DDL / Migration
// ---------------------------------------------------------------------------

fn schema_v1() -> Schema {
    Schema::builder()
        .set(
            EntitySet::new("ThingSet", "Thing")
                .drafts()
                .column(Column::new("Name", DataType::String { max_length: Some(10) })),
        )
        .build()
        .unwrap()
}

fn schema_v2() -> Schema {
    Schema::builder()
        .set(
            EntitySet::new("ThingSet", "Thing")
                .drafts()
                .column(Column::new("Name", DataType::String { max_length: Some(10) }))
                .column(Column::new("Note", DataType::String { max_length: Some(99) })),
        )
        .build()
        .unwrap()
}

#[test]
fn additive_migration_adds_columns_and_keeps_data() {
    let dir = TempDir::new().unwrap();
    {
        let store = RelationalStore::open(dir.path(), Arc::new(StaticSchema::new(schema_v1())))
            .expect("v1");
        store
            .create_entity(
                "ThingSet",
                &json!({"ID": uuid(1), "IsActiveEntity": true, "Name": "alt"}),
            )
            .unwrap();
    }

    let store = RelationalStore::open(
        dir.path(),
        Arc::new(StaticSchema::with_revision(schema_v2(), 2)),
    )
    .expect("v2 migriert");

    let read = store
        .read_entity("ThingSet", &active(1), &ODataQuery::new())
        .expect("Bestandsdaten ueberleben");
    assert_eq!(read["Name"], json!("alt"));
    assert_eq!(read["Note"], json!(null), "neue Spalte ist NULL");

    store
        .patch_entity("ThingSet", &active(1), &json!({"Note": "neu"}))
        .expect("neue Spalte ist beschreibbar");
}

#[test]
fn destructive_changes_are_reported_not_applied() {
    let dir = TempDir::new().unwrap();
    {
        RelationalStore::open(dir.path(), Arc::new(StaticSchema::with_revision(schema_v2(), 2)))
            .expect("v2");
    }
    // Zurueck auf v1: 'Note' ist im Schema weg, in der Datenbank noch da.
    let store =
        RelationalStore::open(dir.path(), Arc::new(StaticSchema::new(schema_v1()))).expect("v1");
    let pending = store.pending_destructive_changes().unwrap();
    assert!(
        pending.iter().any(|p| p.contains("Note")),
        "Spaltenverlust muss gemeldet werden: {pending:?}"
    );
}

#[test]
fn reopening_with_the_same_revision_is_a_no_op() {
    let dir = TempDir::new().unwrap();
    let source = Arc::new(StaticSchema::new(conformance::schema()));
    let store = RelationalStore::open(dir.path(), source.clone()).unwrap();
    assert!(!store.refresh_schema().unwrap(), "keine Migration noetig");
}

// ---------------------------------------------------------------------------
// Typen
// ---------------------------------------------------------------------------

#[test]
fn decimals_keep_their_precision() {
    let schema = Schema::builder()
        .set(
            EntitySet::new("MoneySet", "Money").drafts().column(Column::new(
                "Amount",
                DataType::Decimal {
                    precision: 15,
                    scale: 2,
                },
            )),
        )
        .build()
        .unwrap();
    let store = RelationalStore::open_in_memory(Arc::new(StaticSchema::new(schema))).unwrap();
    store
        .create_entity(
            "MoneySet",
            &json!({"ID": uuid(1), "IsActiveEntity": true, "Amount": 1234.56}),
        )
        .unwrap();
    let read = store
        .read_entity("MoneySet", &active(1), &ODataQuery::new())
        .unwrap();
    assert_eq!(read["Amount"], json!(1234.56));
}

#[test]
fn count_state_distinguishes_drafts() {
    let store = seeded();
    assert_eq!(store.count_state(ORDERS, true), 1);
    assert_eq!(store.count_state(ORDERS, false), 0);
    store.draft_edit(ORDERS, &active(1)).unwrap();
    assert_eq!(store.count_state(ORDERS, false), 1);
}

#[test]
fn navigation_builder_is_not_supported() {
    // Metadaten kommen ausschliesslich ueber die SchemaSource.
    let store = seeded();
    store.set_navigations(vec![]); // wirkungslos, aber kein Fehler
    assert_eq!(store.count_state(ORDERS, true), 1);
}

#[test]
fn sibling_of_missing_record_is_an_error() {
    let store = seeded();
    assert!(store.read_sibling_entity(ORDERS, &active(1)).is_err());
    let _ = EntityKey::new(uuid(1), true);
}
