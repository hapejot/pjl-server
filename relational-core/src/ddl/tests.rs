use super::*;
use crate::catalog::FakeCatalog;
use crate::dialect::SQLITE;

fn schema() -> Schema {
    data_store::conformance::schema()
}

/// Auf einer leeren Datenbank entsteht fuer jedes Set ein CREATE TABLE -
/// und kein einziges ALTER.
#[tokio::test]
async fn empty_database_is_created_from_scratch() {
    let plan = migration_plan(&FakeCatalog::default(), &SQLITE, &schema())
        .await
        .unwrap();
    assert!(plan
        .iter()
        .any(|s| s.contains(r#"CREATE TABLE IF NOT EXISTS "OrderSet""#)));
    assert!(!plan.iter().any(|s| s.starts_with("ALTER TABLE")));
}

#[tokio::test]
async fn draft_mode_changes_require_explicit_migration() {
    for enabled in [false, true] {
        let mut set = EntitySet::new("ThingSet", "Thing");
        if enabled {
            set = set.drafts();
        }
        let schema = Schema::builder().set(set).build().unwrap();
        let columns = if enabled {
            vec!["ID"]
        } else {
            vec!["ID", "IsActiveEntity"]
        };
        let catalog = FakeCatalog::default().with_table("ThingSet", &columns);
        let error = migration_plan(&catalog, &SQLITE, &schema)
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("Primaerschluesselmigration"),
            "{error}"
        );
    }
}

/// Der Parent muss vor dem Kind stehen, sonst greift der Fremdschluessel
/// ins Leere.
#[tokio::test]
async fn parents_are_created_before_their_children() {
    let plan = migration_plan(&FakeCatalog::default(), &SQLITE, &schema())
        .await
        .unwrap();
    let position = |needle: &str| {
        plan.iter()
            .position(|s| s.contains(&format!("CREATE TABLE IF NOT EXISTS \"{needle}\"")))
            .unwrap_or_else(|| panic!("{needle} fehlt im Plan"))
    };
    assert!(position("OrderSet") < position("ItemSet"));
}

/// Eine bestehende Tabelle wird ergaenzt, nicht neu angelegt.
#[tokio::test]
async fn missing_columns_are_added_to_an_existing_table() {
    let catalog = FakeCatalog::default().with_table("OrderSet", &["ID", "IsActiveEntity"]);
    let plan = migration_plan(&catalog, &SQLITE, &schema()).await.unwrap();
    assert!(!plan
        .iter()
        .any(|s| s.contains(r#"CREATE TABLE IF NOT EXISTS "OrderSet""#)));
    assert!(plan
        .iter()
        .any(|s| s == r#"ALTER TABLE "OrderSet" ADD COLUMN "Amount" INTEGER"#));
    // Vorhandene Spalten bleiben unangetastet.
    assert!(!plan.iter().any(|s| s.contains(r#"ADD COLUMN "ID""#)));
}

/// Spalten, die nur noch in der Datenbank stehen, werden gemeldet - aber
/// nicht angefasst.
#[tokio::test]
async fn dropped_columns_are_reported_not_removed() {
    let catalog =
        FakeCatalog::default().with_table("OrderSet", &["ID", "IsActiveEntity", "Vergessen"]);
    let plan = migration_plan(&catalog, &SQLITE, &schema()).await.unwrap();
    assert!(!plan.iter().any(|s| s.contains("DROP")));

    let findings = pending_destructive_changes(&catalog, &SQLITE, &schema())
        .await
        .unwrap();
    assert_eq!(
        findings,
        vec!["OrderSet.Vergessen existiert in der Datenbank, aber nicht im Schema"]
    );
}

/// Eine Assoziation darf keinen Fremdschluessel mehr haben: der bestehende
/// wird entfernt, auch wenn er einmal als Komposition angelegt wurde.
#[tokio::test]
async fn stale_association_constraint_is_dropped() {
    let catalog = FakeCatalog::default()
        .with_table("OrderSet", &["ID", "IsActiveEntity", "Customer_ID"])
        .with_foreign_key(
            "OrderSet",
            "alt_fk",
            &["Customer_ID", "IsActiveEntity"],
            "CustomerSet",
        );
    let plan = foreign_key_plan(&catalog, &schema()).await.unwrap();
    assert_eq!(
        plan,
        vec![r#"ALTER TABLE "OrderSet" DROP CONSTRAINT "alt_fk""#.to_string()]
    );
}

/// Eine Komposition ohne Fremdschluessel bekommt einen - mit CASCADE.
#[tokio::test]
async fn missing_composition_constraint_is_added() {
    let catalog =
        FakeCatalog::default().with_table("ItemSet", &["ID", "IsActiveEntity", "Order_ID"]);
    let plan = foreign_key_plan(&catalog, &schema()).await.unwrap();
    assert_eq!(plan.len(), 1, "{plan:?}");
    assert!(plan[0]
        .starts_with(r#"ALTER TABLE "ItemSet" ADD CONSTRAINT "fk_ItemSet_Order_ID" FOREIGN KEY"#));
    assert!(plan[0].ends_with("ON DELETE CASCADE"));
}

/// Stimmt der bestehende Fremdschluessel bereits, bleibt alles unangetastet.
#[tokio::test]
async fn matching_composition_constraint_is_left_alone() {
    let catalog = FakeCatalog::default()
        .with_table("ItemSet", &["ID", "IsActiveEntity", "Order_ID"])
        .with_foreign_key("ItemSet", "x", &["IsActiveEntity", "Order_ID"], "OrderSet");
    assert!(foreign_key_plan(&catalog, &schema())
        .await
        .unwrap()
        .is_empty());
}

/// Fremdschluessel auf Spalten, die kein verwaltetes Feld sind, bleiben.
#[tokio::test]
async fn unmanaged_constraints_are_left_alone() {
    let catalog = FakeCatalog::default()
        .with_table(
            "OrderSet",
            &["ID", "IsActiveEntity", "Customer_ID", "Eigen"],
        )
        .with_foreign_key("OrderSet", "eigen_fk", &["Eigen"], "Anderes");
    assert!(foreign_key_plan(&catalog, &schema())
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn legacy_text_column_is_translated_then_dropped() {
    use data_store::schema::{Column, DataType as DT, EntitySet, LegacyValueColumn};
    let mut set = EntitySet::new("ProjectSet", "Project").drafts();
    set = set.column(Column::new("status_ID", DT::Guid));
    set.legacy_value_columns.push(LegacyValueColumn {
        old_column: "status".into(),
        fk_column: "status_ID".into(),
        mapping: vec![(
            "open".into(),
            uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
        )],
    });
    let schema = Schema::builder().set(set).build().unwrap();
    let catalog =
        FakeCatalog::default().with_table("ProjectSet", &["ID", "IsActiveEntity", "status"]);
    let plan = migration_plan(&catalog, &SQLITE, &schema).await.unwrap();
    let update = plan
        .iter()
        .position(|s| s.starts_with("UPDATE \"ProjectSet\""))
        .expect("UPDATE");
    let drop = plan
        .iter()
        .position(|s| s.contains("DROP COLUMN \"status\""))
        .expect("DROP");
    assert!(update < drop, "Uebersetzung muss vor dem Entfernen laufen");
    assert!(
        plan[update].contains("WHEN 'open' THEN CAST('00000000-0000-0000-0000-000000000001' AS")
    );
}

#[tokio::test]
async fn fresh_legacy_free_table_has_no_drop() {
    use data_store::schema::EntitySet;
    let set = EntitySet::new("ProjectSet", "Project").drafts();
    let schema = Schema::builder().set(set).build().unwrap();
    let catalog = FakeCatalog::default().with_table("ProjectSet", &["ID", "IsActiveEntity"]);
    let plan = migration_plan(&catalog, &SQLITE, &schema).await.unwrap();
    assert!(!plan.iter().any(|s| s.contains("DROP COLUMN")));
}

/// Kompositionen bekommen einen Fremdschluessel inklusive
/// IsActiveEntity; lose Assoziationen bekommen keinen.
#[test]
fn only_compositions_get_a_foreign_key() {
    let schema = schema();
    let items = create_table_sql(&SQLITE, &schema, schema.set("ItemSet").unwrap()).unwrap();
    assert!(
        items.contains(r#"FOREIGN KEY ("Order_ID", "IsActiveEntity")"#),
        "{items}"
    );
    let orders = create_table_sql(&SQLITE, &schema, schema.set("OrderSet").unwrap()).unwrap();
    assert!(!orders.contains("Customer_ID\") REFERENCES"), "{orders}");
}
