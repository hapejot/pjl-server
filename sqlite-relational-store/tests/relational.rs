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

async fn store() -> RelationalStore {
    RelationalStore::open_in_memory(Arc::new(StaticSchema::new(conformance::schema()))).await
        .expect("store")
}

async fn seeded() -> RelationalStore {
    let store = store().await;
    store
        .create_entity(
            "CustomerSet",
            &json!({"ID": uuid(100), "IsActiveEntity": true, "Name": "Mueller GmbH"}),
        ).await
        .unwrap();
    store
        .create_entity(
            ORDERS,
            &json!({"ID": uuid(1), "IsActiveEntity": true, "Customer_ID": uuid(100),
                    "Amount": 150, "Status": "New"}),
        ).await
        .unwrap();
    for (id, material) in [(10, "M-100"), (11, "M-200")] {
        store
            .create_entity(
                ITEMS,
                &json!({"ID": uuid(id), "IsActiveEntity": true, "Order_ID": uuid(1),
                        "Material": material, "Qty": 1}),
            ).await
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

#[tokio::test]
async fn unknown_field_on_create_is_rejected() {
    let store = store().await;
    let err = store
        .create_entity(
            ORDERS,
            &json!({"ID": uuid(1), "IsActiveEntity": true, "Bogus": 1}),
        ).await
        .expect_err("unbekanntes Feld muss auffallen");
    assert!(err.to_string().contains("Bogus"), "{err}");
}

#[tokio::test]
async fn unknown_field_on_patch_is_rejected() {
    let store = seeded().await;
    assert!(store
        .patch_entity(ORDERS, &active(1), &json!({"Bogus": 1})).await
        .is_err());
}

#[tokio::test]
async fn unknown_entity_set_is_rejected() {
    let store = store().await;
    assert!(store.get_collection("NichtDaSet", &ODataQuery::new()).await.is_err());
}

#[tokio::test]
async fn type_mismatch_is_reported() {
    let store = store().await;
    let err = store
        .create_entity(
            ORDERS,
            &json!({"ID": uuid(1), "IsActiveEntity": true, "Amount": {"nested": true}}),
        ).await
        .expect_err("Objekt passt nicht in eine Int-Spalte");
    assert!(err.to_string().contains("Amount"), "{err}");
}

// ---------------------------------------------------------------------------
// SQL-Uebersetzung
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filter_on_unknown_column_is_an_error_not_an_empty_result() {
    let store = seeded().await;
    let query = ODataQuery::new().filter(bin(
        BinOp::Eq,
        Expr::Identifier("Nope".into()),
        Expr::String("x".into()),
    ));
    let err = store
        .get_collection(ORDERS, &query).await
        .expect_err("Tippfehler darf nicht als 'kein Treffer' durchgehen");
    assert!(err.to_string().contains("Nope"), "{err}");
}

#[tokio::test]
async fn untranslatable_function_fails_loudly() {
    let store = seeded().await;
    let query = ODataQuery::new().filter(Expr::FunctionCall {
        name: "floor".into(),
        args: vec![Expr::Identifier("Amount".into())],
    });
    let err = store
        .get_collection(ORDERS, &query).await
        .expect_err("floor kennt SQLite nicht - lieber Fehler als falsches Ergebnis");
    assert!(err.to_string().contains("floor"), "{err}");
}

#[tokio::test]
async fn path_expressions_in_filter_are_rejected() {
    let store = seeded().await;
    let query = ODataQuery::new().filter(bin(
        BinOp::Eq,
        Expr::Identifier("Customer/Name".into()),
        Expr::String("x".into()),
    ));
    assert!(store.get_collection(ORDERS, &query).await.is_err());
}

fn lambda_query(filter: &str) -> ODataQuery {
    ODataQuery::new().filter(Expr::parse(filter).expect("parst"))
}

#[tokio::test]
async fn any_lambda_selects_parents_with_a_matching_child() {
    let store = seeded().await;
    let hit = lambda_query("_Items/any(i: i/Material eq 'M-100')");
    assert_eq!(store.get_collection(ORDERS, &hit).await.unwrap().len(), 1);
    let miss = lambda_query("_Items/any(i: i/Material eq 'M-999')");
    assert_eq!(store.get_collection(ORDERS, &miss).await.unwrap().len(), 0);
}

#[tokio::test]
async fn all_lambda_requires_every_child_to_match() {
    let store = seeded().await;
    let all_qty_one = lambda_query("_Items/all(i: i/Qty eq 1)");
    assert_eq!(store.get_collection(ORDERS, &all_qty_one).await.unwrap().len(), 1);
    let all_m100 = lambda_query("_Items/all(i: i/Material eq 'M-100')");
    assert_eq!(store.get_collection(ORDERS, &all_m100).await.unwrap().len(), 0);
}

#[tokio::test]
async fn lambda_over_empty_collection_any_false_all_true() {
    let store = seeded().await;
    store
        .create_entity(
            ORDERS,
            &json!({"ID": uuid(2), "IsActiveEntity": true, "Customer_ID": uuid(100),
                    "Amount": 5, "Status": "Open"}),
        ).await
        .unwrap();
    // Auftrag 2 hat keine Positionen.
    let any = lambda_query("_Items/any(i: i/Qty eq 1)");
    assert_eq!(store.get_collection(ORDERS, &any).await.unwrap().len(), 1);
    let all = lambda_query("_Items/all(i: i/Qty eq 1)");
    assert_eq!(store.get_collection(ORDERS, &all).await.unwrap().len(), 2);
}

#[tokio::test]
async fn lambda_predicate_can_follow_to_one_navigations() {
    let store = seeded().await;
    let query = lambda_query("_Items/any(i: i/Order/Customer/Name eq 'Mueller GmbH')");
    assert_eq!(store.get_collection(ORDERS, &query).await.unwrap().len(), 1);
}

#[tokio::test]
async fn sibling_lambdas_are_anded_on_the_same_parent() {
    let store = seeded().await;
    store
        .create_entity(
            ORDERS,
            &json!({"ID": uuid(2), "IsActiveEntity": true, "Customer_ID": uuid(100),
                    "Amount": 5, "Status": "Open"}),
        ).await
        .unwrap();
    store
        .create_entity(
            ITEMS,
            &json!({"ID": uuid(12), "IsActiveEntity": true, "Order_ID": uuid(2),
                    "Material": "M-100", "Qty": 1}),
        ).await
        .unwrap();
    // Auftrag 1 hat M-100 und M-200, Auftrag 2 nur M-100.
    let both = lambda_query(
        "_Items/any(i: i/Material eq 'M-100') and _Items/any(j: j/Material eq 'M-200')",
    );
    assert_eq!(store.get_collection(ORDERS, &both).await.unwrap().len(), 1);
    let same_var = lambda_query(
        "_Items/any(i: i/Material eq 'M-100') and _Items/any(i: i/Material eq 'M-200')",
    );
    assert_eq!(store.get_collection(ORDERS, &same_var).await.unwrap().len(), 1);
    let one = lambda_query("_Items/any(i: i/Material eq 'M-100')");
    assert_eq!(store.get_collection(ORDERS, &one).await.unwrap().len(), 2);
}

#[tokio::test]
async fn string_functions_translate_to_sql() {
    let store = seeded().await;
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
        let result = store.get_collection(ORDERS, &query).await.expect(name);
        assert_eq!(result.len(), 1, "{name} sollte 'New' treffen");
    }
}

/// Werte gehen als Parameter in die Anweisung - ein Anfuehrungszeichen im
/// Suchbegriff darf nichts kaputt machen.
#[tokio::test]
async fn filter_values_are_bound_not_interpolated() {
    let store = seeded().await;
    let query = ODataQuery::new().filter(bin(
        BinOp::Eq,
        Expr::Identifier("Status".into()),
        Expr::String("'; DROP TABLE \"OrderSet\"; --".into()),
    ));
    assert_eq!(store.get_collection(ORDERS, &query).await.unwrap().len(), 0);
    // Tabelle steht noch.
    assert_eq!(store.get_collection(ORDERS, &ODataQuery::new()).await.unwrap().len(), 1);
}

#[tokio::test]
async fn orderby_on_unknown_column_is_an_error() {
    let store = seeded().await;
    let query = ODataQuery::new().order_by("Nope", SortDirection::Asc);
    assert!(store.get_collection(ORDERS, &query).await.is_err());
}

#[tokio::test]
async fn not_and_null_comparison_translate() {
    let store = seeded().await;
    let query = ODataQuery::new().filter(Expr::UnaryNot(Box::new(bin(
        BinOp::Eq,
        Expr::Identifier("Status".into()),
        Expr::String("New".into()),
    ))));
    assert_eq!(store.get_collection(ORDERS, &query).await.unwrap().len(), 0);

    let is_null = ODataQuery::new().filter(bin(
        BinOp::Eq,
        Expr::Identifier("Status".into()),
        Expr::Null,
    ));
    assert_eq!(store.get_collection(ORDERS, &is_null).await.unwrap().len(), 0);
}

// ---------------------------------------------------------------------------
// Fremdschluessel und Kaskaden
// ---------------------------------------------------------------------------

/// Beweist, dass die Kaskade die Datenbank macht: `delete_entity_tree_only`
/// setzt genau ein DELETE ab, die Kinder verschwinden trotzdem.
#[tokio::test]
async fn composition_cascade_is_enforced_by_the_database() {
    let store = seeded().await;
    assert_eq!(store.count_state(ITEMS, true), 2);
    store
        .delete_entity_tree_only(ORDERS, &active(1)).await
        .expect("delete");
    assert_eq!(
        store.count_state(ITEMS, true),
        0,
        "ON DELETE CASCADE muss die Positionen mitnehmen"
    );
}

#[tokio::test]
async fn child_under_missing_parent_is_rejected() {
    let store = store().await;
    let err = store
        .create_entity(
            ITEMS,
            &json!({"ID": uuid(10), "IsActiveEntity": true, "Order_ID": uuid(999),
                    "Material": "M-1", "Qty": 1}),
        ).await
        .expect_err("Position ohne Auftrag");
    assert!(format!("{err}").contains("999") || format!("{err}").contains("nicht gefunden"));
}

/// Der Fremdschluessel schliesst `IsActiveEntity` ein: ein Draft-Kind kann
/// nicht unter einem rein aktiven Parent haengen.
#[tokio::test]
async fn draft_child_requires_a_draft_parent() {
    let store = seeded().await;
    let err = store
        .create_entity(
            ITEMS,
            &json!({"ID": uuid(13), "IsActiveEntity": false, "Order_ID": uuid(1),
                    "Material": "M-3", "Qty": 1}),
        ).await
        .expect_err("kein Draft-Parent vorhanden");
    assert!(format!("{err}").len() > 0);
}

// ---------------------------------------------------------------------------
// Abgeleitete Draft-Flags
// ---------------------------------------------------------------------------

#[tokio::test]
async fn has_draft_entity_is_computed_from_the_sibling() {
    let store = seeded().await;
    let before = store.read_entity(ORDERS, &active(1), &ODataQuery::new()).await.unwrap();
    assert!(!before.has_draft_entity());

    store.draft_edit(ORDERS, &active(1)).await.unwrap();

    let after = store.read_entity(ORDERS, &active(1), &ODataQuery::new()).await.unwrap();
    assert!(
        after.has_draft_entity(),
        "abgeleitet aus der Existenz des Drafts, nicht gespeichert"
    );
    let draft_row = store.read_entity(ORDERS, &draft(1), &ODataQuery::new()).await.unwrap();
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

#[tokio::test]
async fn additive_migration_adds_columns_and_keeps_data() {
    let dir = TempDir::new().unwrap();
    {
        let store = RelationalStore::open(dir.path(), Arc::new(StaticSchema::new(schema_v1()))).await
            .expect("v1");
        store
            .create_entity(
                "ThingSet",
                &json!({"ID": uuid(1), "IsActiveEntity": true, "Name": "alt"}),
            ).await
            .unwrap();
        store.persist().unwrap();
    }

    let store = RelationalStore::open(
        dir.path(),
        Arc::new(StaticSchema::with_revision(schema_v2(), 2)),
    ).await
    .expect("v2 migriert");

    let read = store
        .read_entity("ThingSet", &active(1), &ODataQuery::new()).await
        .expect("Bestandsdaten ueberleben");
    assert_eq!(read["Name"], json!("alt"));
    assert_eq!(read["Note"], json!(null), "neue Spalte ist NULL");

    store
        .patch_entity("ThingSet", &active(1), &json!({"Note": "neu"})).await
        .expect("neue Spalte ist beschreibbar");
}

#[tokio::test]
async fn destructive_changes_are_reported_not_applied() {
    let dir = TempDir::new().unwrap();
    {
        let store = RelationalStore::open(dir.path(), Arc::new(StaticSchema::with_revision(schema_v2(), 2))).await
            .expect("v2");
        store.persist().unwrap();
    }
    // Zurueck auf v1: 'Note' ist im Schema weg, in der Datenbank noch da.
    let store =
        RelationalStore::open(dir.path(), Arc::new(StaticSchema::new(schema_v1()))).await.expect("v1");
    let pending = store.pending_destructive_changes().await.unwrap();
    assert!(
        pending.iter().any(|p| p.contains("Note")),
        "Spaltenverlust muss gemeldet werden: {pending:?}"
    );
}

#[tokio::test]
async fn reopening_with_the_same_revision_is_a_no_op() {
    let dir = TempDir::new().unwrap();
    let source = Arc::new(StaticSchema::new(conformance::schema()));
    let store = RelationalStore::open(dir.path(), source.clone()).await.unwrap();
    assert!(!store.refresh_schema().await.unwrap(), "keine Migration noetig");
}

// ---------------------------------------------------------------------------
// Typen
// ---------------------------------------------------------------------------

#[tokio::test]
async fn decimals_keep_their_precision() {
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
    let store = RelationalStore::open_in_memory(Arc::new(StaticSchema::new(schema))).await.unwrap();
    store
        .create_entity(
            "MoneySet",
            &json!({"ID": uuid(1), "IsActiveEntity": true, "Amount": 1234.56}),
        ).await
        .unwrap();
    let read = store
        .read_entity("MoneySet", &active(1), &ODataQuery::new()).await
        .unwrap();
    assert_eq!(read["Amount"], json!(1234.56));
}

#[tokio::test]
async fn count_state_distinguishes_drafts() {
    let store = seeded().await;
    assert_eq!(store.count_state(ORDERS, true), 1);
    assert_eq!(store.count_state(ORDERS, false), 0);
    store.draft_edit(ORDERS, &active(1)).await.unwrap();
    assert_eq!(store.count_state(ORDERS, false), 1);
}

#[tokio::test]
async fn navigation_builder_is_not_supported() {
    // Metadaten kommen ausschliesslich ueber die SchemaSource.
    let store = seeded().await;
    store.set_navigations(vec![]); // wirkungslos, aber kein Fehler
    assert_eq!(store.count_state(ORDERS, true), 1);
}

#[tokio::test]
async fn sibling_of_missing_record_is_an_error() {
    let store = seeded().await;
    assert!(store.read_sibling_entity(ORDERS, &active(1)).await.is_err());
    let _ = EntityKey::new(uuid(1), true);
}

// ---------------------------------------------------------------------------
// Transaktionen
// ---------------------------------------------------------------------------

/// Schema mit einer Pflichtspalte, damit ein Schreibversuch gezielt an der
/// Datenbank scheitern kann.
fn strict_schema() -> Schema {
    Schema::builder()
        .set(
            EntitySet::new("TicketSet", "Ticket")
                .drafts()
                .column(Column::new("Titel", DataType::String { max_length: None }).required()),
        )
        .build()
        .expect("Schema")
}

async fn strict_store() -> RelationalStore {
    RelationalStore::open_in_memory(Arc::new(StaticSchema::new(strict_schema()))).await.expect("store")
}

/// Ein Draft entsteht in zwei Anweisungen: erst die Verwaltungsdaten, dann
/// der Satz selbst. Scheitert die zweite, darf die erste nicht stehen
/// bleiben - ein Draft-Kopf ohne Satz ist von aussen nicht mehr erreichbar
/// und damit auch nicht mehr aufraeumbar.
#[tokio::test]
async fn a_failed_create_leaves_no_orphaned_draft_administration() {
    let store = strict_store().await;
    assert_eq!(store.count_draft_admin(), 0);

    let result = store.create_entity(
        "TicketSet",
        &json!({"ID": uuid(1), "IsActiveEntity": false}),
    ).await;
    assert!(result.is_err(), "Pflichtspalte fehlt, das muss scheitern");

    assert_eq!(
        store.count_draft_admin(),
        0,
        "die Verwaltungsdaten des gescheiterten Drafts sind zurueckgerollt"
    );
    assert_eq!(store.count_state("TicketSet", false), 0);
}

/// Der Gegenbeweis: gelingt das Anlegen, stehen beide Teile.
#[tokio::test]
async fn a_successful_create_writes_record_and_administration_together() {
    let store = strict_store().await;
    store
        .create_entity(
            "TicketSet",
            &json!({"ID": uuid(1), "IsActiveEntity": false, "Titel": "Drucker klemmt"}),
        ).await
        .expect("Anlegen gelingt");
    assert_eq!(store.count_state("TicketSet", false), 1);
    assert_eq!(store.count_draft_admin(), 1);
}

/// Ein gescheitertes Patch darf den gespeicherten Satz nicht halb
/// veraendert zuruecklassen.
#[tokio::test]
async fn a_failed_patch_changes_nothing() {
    let store = strict_store().await;
    store
        .create_entity(
            "TicketSet",
            &json!({"ID": uuid(1), "IsActiveEntity": false, "Titel": "Drucker klemmt"}),
        ).await
        .unwrap();

    let key = EntityKey::new(uuid(1), false);
    let result = store.patch_entity("TicketSet", &key, &json!({"Titel": null})).await;
    assert!(result.is_err(), "Pflichtspalte darf nicht geleert werden");

    let record = store
        .read_entity("TicketSet", &key, &ODataQuery::new()).await
        .unwrap();
    assert_eq!(record.get("Titel"), Some(json!("Drucker klemmt")));
    assert_eq!(store.count_draft_admin(), 1);
}

#[tokio::test]
async fn select_keeps_is_active_entity_of_active_records() {
    // Regression: `$select=ID` darf den aktiven Satz nicht als Draft ausgeben.
    let store = seeded().await;
    let query = ODataQuery::new().select("ID");
    let rows = store.get_collection(ORDERS, &query).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].is_active_entity());
    assert!(!rows[0].has_draft_entity());
}

#[tokio::test]
async fn filter_on_computed_draft_flags_is_accepted() {
    // Fiori schickt `HasActiveEntity eq false and HasDraftEntity eq false`.
    let store = seeded().await;
    let filter = Expr::BinaryOp {
        op: BinOp::And,
        left: Box::new(bin(BinOp::Eq, Expr::Identifier("HasActiveEntity".into()), Expr::Bool(false))),
        right: Box::new(bin(BinOp::Eq, Expr::Identifier("HasDraftEntity".into()), Expr::Bool(false))),
    };
    let query = ODataQuery::new().filter(filter);
    let rows = store.get_collection(ORDERS, &query).await.expect("Filter ist gueltig");
    assert_eq!(rows.len(), 1);
}

#[tokio::test]
async fn select_accepts_computed_draft_flags() {
    let store = seeded().await;
    let query = ODataQuery::new()
        .select("ID")
        .select("IsActiveEntity")
        .select("HasActiveEntity")
        .select("HasDraftEntity");
    let rows = store.get_collection(ORDERS, &query).await.expect("$select ist gueltig");
    assert_eq!(rows.len(), 1);
    assert!(rows[0].is_active_entity());
}
