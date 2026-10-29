//! DDL-Erzeugung und Migrationsplanung.
//!
//! Das Schema ist die Wahrheit; die Datenbank wird darauf gebracht. Additive
//! Aenderungen laufen automatisch, destruktive brauchen einen expliziten
//! Aufruf - ein Serverstart soll keine Spalten verlieren.
//!
//! Das Modul fuehrt nichts aus. Es erzeugt Anweisungen und ueberlaesst das
//! Absetzen dem Store. Der Zustand der Datenbank kommt ueber
//! [`crate::catalog::Catalog`] herein, die Schreibweise ueber
//! [`crate::dialect::Dialect`]. Damit ist die Migration ohne laufende
//! Datenbank pruefbar - und der Teil, der zwischen Backends gleich bleibt,
//! ist der weitaus groessere.

use std::collections::HashSet;

use data_store::schema::{
    Column, ColumnRole, DataType, EntitySet, Schema, ID_COLUMN, IS_ACTIVE_COLUMN,
};
use data_store::StoreError;
use tracing::info;

use crate::catalog::Catalog;
use crate::dialect::Dialect;

/// Spalte mit den Draft-Verwaltungsdaten (FK auf [`DRAFT_ADMIN_TABLE`]).
pub const DRAFT_ADMIN_COLUMN: &str = "DraftAdministrativeData_ID";
/// Validierungsmeldungen des Draft-Flows, als JSON abgelegt.
pub const DRAFT_MESSAGES_COLUMN: &str = "DraftMessages";
/// Tabelle der Draft-Verwaltungsdaten.
pub const DRAFT_ADMIN_TABLE: &str = "DraftAdministrativeData";
/// Ablage der angewandten Schema-Revision.
pub const REVISION_TABLE: &str = "_schema_revision";

/// Bezeichner sicher quoten.
pub fn quote(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

fn column_ddl(dialect: &dyn Dialect, column: &Column) -> String {
    let mut ddl = format!(
        "{} {}",
        quote(&column.name),
        dialect.column_type(&column.data_type)
    );
    if !column.nullable {
        ddl.push_str(" NOT NULL");
    }
    if let Some(default) = &column.default {
        ddl.push_str(&format!(" DEFAULT {}", literal(dialect, default)));
    }
    ddl
}

fn literal(dialect: &dyn Dialect, value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "NULL".into(),
        serde_json::Value::Bool(b) => dialect.bool_literal(*b).into(),
        serde_json::Value::Number(n) => n.to_string(),
        other => format!(
            "'{}'",
            other.as_str().unwrap_or_default().replace('\'', "''")
        ),
    }
}

/// Interne Spalten, die jedes draft-faehige Set zusaetzlich braucht.
pub fn internal_columns(dialect: &dyn Dialect, set: &EntitySet) -> Vec<(String, String)> {
    if set.draft_enabled {
        let text = dialect
            .column_type(&DataType::String { max_length: None })
            .to_string();
        vec![
            (DRAFT_ADMIN_COLUMN.to_string(), text.clone()),
            (DRAFT_MESSAGES_COLUMN.to_string(), text),
        ]
    } else {
        vec![]
    }
}

/// Der Fremdschluessel, den die Schema-Seite fuer ein Set verlangt.
///
/// Nur Kompositionen haben einen. Lose Assoziationen bekommen keinen
/// Constraint - sonst muesste ein Draft-Auftrag auf einen Draft-Kunden zeigen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKeySpec {
    /// Name, unter dem `ALTER TABLE ... ADD CONSTRAINT` ihn anlegt.
    pub name: String,
    /// Spalten der eigenen Tabelle, sortiert.
    pub columns: Vec<String>,
    pub target_table: String,
    /// `FOREIGN KEY (...) REFERENCES ... ON DELETE CASCADE`, ohne Constraint-Namen.
    pub clause: String,
}

pub fn desired_foreign_key(schema: &Schema, set: &EntitySet) -> Result<Option<ForeignKeySpec>, StoreError> {
    let Some(parent) = set.composition_parent() else {
        return Ok(None);
    };
    let target = schema
        .set(&parent.target_set)
        .map_err(|e| StoreError::Metadata(format!("DDL: {e}")))?;
    let fk_column = parent.foreign_key.column();
    let (columns, referenced) = if set.draft_enabled && target.draft_enabled {
        (
            vec![fk_column.to_string(), IS_ACTIVE_COLUMN.to_string()],
            vec![ID_COLUMN.to_string(), IS_ACTIVE_COLUMN.to_string()],
        )
    } else {
        (vec![fk_column.to_string()], vec![ID_COLUMN.to_string()])
    };
    let clause = format!(
        "FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE CASCADE",
        columns.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", "),
        quote(&target.name),
        referenced.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", "),
    );
    let mut sorted = columns.clone();
    sorted.sort();
    Ok(Some(ForeignKeySpec {
        name: format!("fk_{}_{}", set.name, fk_column),
        columns: sorted,
        target_table: target.name.clone(),
        clause,
    }))
}

/// `CREATE TABLE` fuer ein Entity-Set. Fremdschluessel nur fuer Kompositionen,
/// siehe [`desired_foreign_key`].
pub fn create_table_sql(
    dialect: &dyn Dialect,
    schema: &Schema,
    set: &EntitySet,
) -> Result<String, StoreError> {
    let mut parts: Vec<String> = set.columns().map(|c| column_ddl(dialect, c)).collect();
    for (name, ty) in internal_columns(dialect, set) {
        parts.push(format!("{} {}", quote(&name), ty));
    }

    let keys: Vec<String> = set.key_columns().iter().map(|c| quote(&c.name)).collect();
    parts.push(format!("PRIMARY KEY ({})", keys.join(", ")));

    if let Some(fk) = desired_foreign_key(schema, set)? {
        parts.push(fk.clause);
    }

    Ok(format!(
        "CREATE TABLE IF NOT EXISTS {} (\n  {}\n)",
        quote(&set.name),
        parts.join(",\n  ")
    ))
}

/// Gleicht die Fremdschluessel bestehender Tabellen mit der Schema-Seite ab.
///
/// Verwaltet werden die Spalten der to-one-Assoziationen. Ein Fremdschluessel
/// auf einer solchen Spalte, den das Schema nicht (mehr) verlangt, wird
/// entfernt; ein fehlender wird angelegt. Fremdschluessel auf anderen Spalten
/// werden nicht angefasst. Nur fuer Backends, die Constraints aendern koennen.
pub async fn foreign_key_plan(
    catalog: &dyn Catalog,
    schema: &Schema,
) -> Result<Vec<String>, StoreError> {
    let mut plan = vec![];
    for set in schema.sets() {
        if !catalog.table_exists(&set.name).await? {
            continue;
        }
        let managed: HashSet<String> = set
            .associations()
            .filter(|a| a.cardinality == data_store::Cardinality::ToOne)
            .map(|a| a.foreign_key.column().to_string())
            .collect();
        let desired = desired_foreign_key(schema, set)?;
        let existing = catalog.foreign_keys(&set.name).await?;

        let mut has_desired = false;
        for fk in &existing {
            let touches_managed = fk.columns.iter().any(|c| managed.contains(c) && c != IS_ACTIVE_COLUMN);
            if !touches_managed {
                continue;
            }
            let matches = desired.as_ref().is_some_and(|d| {
                d.columns == fk.columns && d.target_table == fk.target_table
            });
            if matches {
                has_desired = true;
                continue;
            }
            info!("Migration: Fremdschluessel {} auf {} entfaellt", fk.name, set.name);
            plan.push(format!(
                "ALTER TABLE {} DROP CONSTRAINT {}",
                quote(&set.name),
                quote(&fk.name)
            ));
        }
        if let Some(d) = desired {
            if !has_desired {
                info!("Migration: Fremdschluessel {} auf {} wird angelegt", d.name, set.name);
                plan.push(format!(
                    "ALTER TABLE {} ADD CONSTRAINT {} {}",
                    quote(&set.name),
                    quote(&d.name),
                    d.clause
                ));
            }
        }
    }
    Ok(plan)
}

/// Indizes auf allen Fremdschluesselspalten.
pub fn index_statements(set: &EntitySet) -> Vec<String> {
    set.columns()
        .filter(|c| matches!(c.role, ColumnRole::ForeignKey { .. }))
        .map(|c| {
            format!(
                "CREATE INDEX IF NOT EXISTS {} ON {} ({})",
                quote(&format!("ix_{}_{}", set.name, c.name)),
                quote(&set.name),
                quote(&c.name)
            )
        })
        .collect()
}

/// Nachtraeglich ergaenzte Spalten sind NULL-bar. Hat die Spalte einen Default
/// im Schema, bekommen bestehende Zeilen diesen Wert.
pub fn add_column_sql(
    table: &str,
    column: &str,
    column_type: &str,
    default: Option<&str>,
) -> String {
    let default = default.map(|d| format!(" DEFAULT {d}")).unwrap_or_default();
    format!(
        "ALTER TABLE {} ADD COLUMN {} {}{}",
        quote(table),
        quote(column),
        column_type,
        default
    )
}

/// DDL der Draft-Verwaltungstabelle.
///
/// Sie steht nicht im Schema, weil sie kein Entity-Set ist - der Typaufbau
/// muss trotzdem ueber den Dialekt laufen: `DraftIsCreatedByMe` ist ein
/// Wahrheitswert, und dessen Speicherung unterscheidet sich.
pub fn draft_admin_ddl(dialect: &dyn Dialect) -> String {
    let text = dialect.column_type(&DataType::String { max_length: None });
    format!(
        "CREATE TABLE IF NOT EXISTS {table} (\n  \
           {uuid} {text} NOT NULL PRIMARY KEY,\n  \
           {created} {flag} NOT NULL DEFAULT {yes},\n  \
           {in_process} {text},\n  \
           {changed} {text}\n)",
        table = quote(DRAFT_ADMIN_TABLE),
        uuid = quote("DraftUUID"),
        created = quote("DraftIsCreatedByMe"),
        in_process = quote("InProcessByUser"),
        changed = quote("LastChangedByUser"),
        flag = dialect.column_type(&DataType::Bool),
        yes = dialect.bool_literal(true),
        text = text,
    )
}

/// DDL der Revisionstabelle.
pub fn revision_table_ddl(dialect: &dyn Dialect) -> String {
    format!(
        "CREATE TABLE IF NOT EXISTS {} (revision {} NOT NULL)",
        quote(REVISION_TABLE),
        dialect.column_type(&DataType::Int64)
    )
}

/// Anweisungen, die die Datenbank auf den Stand des Schemas bringen.
///
/// Additiv: fehlende Tabellen anlegen, fehlende Spalten ergaenzen.
/// Typaenderungen und Spalten-Drops sind **nicht** enthalten - sie brauchen
/// einen Tabellen-Rebuild und damit eine bewusste Entscheidung. Was dabei
/// uebrig bleibt, meldet [`pending_destructive_changes`].
///
/// Die Reihenfolge ist bindend: Kompositionen sind Fremdschluessel, also muss
/// der Parent vor dem Kind stehen.
pub async fn migration_plan(
    catalog: &dyn Catalog,
    dialect: &dyn Dialect,
    schema: &Schema,
) -> Result<Vec<String>, StoreError> {
    let mut plan = vec![draft_admin_ddl(dialect), revision_table_ddl(dialect)];

    for set in schema
        .creation_order()
        .map_err(|e| StoreError::Metadata(format!("DDL: {e}")))?
    {
        if catalog.table_exists(&set.name).await? {
            let existing = catalog.existing_columns(&set.name).await?;
            let mut wanted: Vec<(String, String)> = set
                .columns()
                .map(|c| (c.name.clone(), dialect.column_type(&c.data_type).to_string()))
                .collect();
            wanted.extend(internal_columns(dialect, set));
            for (name, ty) in wanted {
                if existing.contains(&name) {
                    continue;
                }
                info!("Migration: {}.{} wird ergaenzt", set.name, name);
                // Bestehende Zeilen bekommen den DEFAULT der Spalte (z. B. Default User).
                let default = set
                    .columns()
                    .find(|c| c.name == name)
                    .and_then(|c| c.default.as_ref())
                    .map(|v| literal(dialect, v));
                plan.push(add_column_sql(&set.name, &name, &ty, default.as_deref()));
            }
            plan.extend(legacy_value_statements(&existing, dialect, set));
        } else {
            plan.push(create_table_sql(dialect, schema, set)?);
        }

        plan.extend(index_statements(set));
    }
    Ok(plan)
}

/// Uebersetzt Festwert-Codes einer frueheren Text-Spalte in Fremdschluessel-IDs
/// und entfernt danach die alte Spalte. Laeuft nur, solange die alte Spalte existiert.
/// Die Anweisungen stehen in dieser Reihenfolge, die neue Spalte wird vorher ergaenzt.
fn legacy_value_statements(
    existing: &HashSet<String>,
    dialect: &dyn Dialect,
    set: &data_store::schema::EntitySet,
) -> Vec<String> {
    let mut out = Vec::new();
    for legacy in &set.legacy_value_columns {
        if !existing.contains(&legacy.old_column) {
            continue;
        }
        let table = quote(&set.name);
        let old = quote(&legacy.old_column);
        let fk = quote(&legacy.fk_column);
        if !legacy.mapping.is_empty() {
            let id_type = dialect.column_type(&DataType::Guid);
            let whens: String = legacy
                .mapping
                .iter()
                .map(|(code, id)| {
                    format!(
                        " WHEN {} THEN CAST({} AS {})",
                        literal(dialect, &serde_json::Value::String(code.clone())),
                        literal(dialect, &serde_json::Value::String(id.to_string())),
                        id_type
                    )
                })
                .collect();
            out.push(format!(
                "UPDATE {table} SET {fk} = CASE {old}{whens} END WHERE {old} IS NOT NULL AND {fk} IS NULL"
            ));
        }
        out.push(format!("ALTER TABLE {table} DROP COLUMN {old}"));
    }
    out
}

/// Meldet Abweichungen, die [`migration_plan`] bewusst nicht behebt.
pub async fn pending_destructive_changes(
    catalog: &dyn Catalog,
    dialect: &dyn Dialect,
    schema: &Schema,
) -> Result<Vec<String>, StoreError> {
    let mut findings = vec![];
    for set in schema.sets() {
        if !catalog.table_exists(&set.name).await? {
            continue;
        }
        let existing = catalog.existing_columns(&set.name).await?;
        let mut wanted: HashSet<String> = set.columns().map(|c| c.name.clone()).collect();
        wanted.extend(internal_columns(dialect, set).into_iter().map(|(n, _)| n));
        for column in existing.difference(&wanted) {
            findings.push(format!(
                "{}.{} existiert in der Datenbank, aber nicht im Schema",
                set.name, column
            ));
        }
    }
    findings.sort();
    Ok(findings)
}

// -- Anweisungen rund um die Revision ----------------------------------------

pub fn revision_select_sql() -> String {
    format!("SELECT revision FROM {} LIMIT 1", quote(REVISION_TABLE))
}

pub fn revision_delete_sql() -> String {
    format!("DELETE FROM {}", quote(REVISION_TABLE))
}

pub fn revision_insert_sql(dialect: &dyn Dialect) -> String {
    format!(
        "INSERT INTO {} (revision) VALUES ({})",
        quote(REVISION_TABLE),
        dialect.placeholder(1)
    )
}

#[cfg(test)]
mod tests {
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
        let plan = migration_plan(&FakeCatalog::default(), &SQLITE, &schema()).await.unwrap();
        assert!(plan.iter().any(|s| s.contains(r#"CREATE TABLE IF NOT EXISTS "OrderSet""#)));
        assert!(!plan.iter().any(|s| s.starts_with("ALTER TABLE")));
    }

    /// Der Parent muss vor dem Kind stehen, sonst greift der Fremdschluessel
    /// ins Leere.
    #[tokio::test]
    async fn parents_are_created_before_their_children() {
        let plan = migration_plan(&FakeCatalog::default(), &SQLITE, &schema()).await.unwrap();
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
        let catalog = FakeCatalog::default().with_table("OrderSet", &["ID", "Vergessen"]);
        let plan = migration_plan(&catalog, &SQLITE, &schema()).await.unwrap();
        assert!(!plan.iter().any(|s| s.contains("DROP")));

        let findings = pending_destructive_changes(&catalog, &SQLITE, &schema()).await.unwrap();
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
            .with_foreign_key("OrderSet", "alt_fk", &["Customer_ID", "IsActiveEntity"], "CustomerSet");
        let plan = foreign_key_plan(&catalog, &schema()).await.unwrap();
        assert_eq!(plan, vec![r#"ALTER TABLE "OrderSet" DROP CONSTRAINT "alt_fk""#.to_string()]);
    }

    /// Eine Komposition ohne Fremdschluessel bekommt einen - mit CASCADE.
    #[tokio::test]
    async fn missing_composition_constraint_is_added() {
        let catalog = FakeCatalog::default().with_table("ItemSet", &["ID", "IsActiveEntity", "Order_ID"]);
        let plan = foreign_key_plan(&catalog, &schema()).await.unwrap();
        assert_eq!(plan.len(), 1, "{plan:?}");
        assert!(plan[0].starts_with(r#"ALTER TABLE "ItemSet" ADD CONSTRAINT "fk_ItemSet_Order_ID" FOREIGN KEY"#));
        assert!(plan[0].ends_with("ON DELETE CASCADE"));
    }

    /// Stimmt der bestehende Fremdschluessel bereits, bleibt alles unangetastet.
    #[tokio::test]
    async fn matching_composition_constraint_is_left_alone() {
        let catalog = FakeCatalog::default()
            .with_table("ItemSet", &["ID", "IsActiveEntity", "Order_ID"])
            .with_foreign_key("ItemSet", "x", &["IsActiveEntity", "Order_ID"], "OrderSet");
        assert!(foreign_key_plan(&catalog, &schema()).await.unwrap().is_empty());
    }

    /// Fremdschluessel auf Spalten, die kein verwaltetes Feld sind, bleiben.
    #[tokio::test]
    async fn unmanaged_constraints_are_left_alone() {
        let catalog = FakeCatalog::default()
            .with_table("OrderSet", &["ID", "IsActiveEntity", "Customer_ID", "Eigen"])
            .with_foreign_key("OrderSet", "eigen_fk", &["Eigen"], "Anderes");
        assert!(foreign_key_plan(&catalog, &schema()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn legacy_text_column_is_translated_then_dropped() {
        use data_store::schema::{EntitySet, LegacyValueColumn, Column, DataType as DT};
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
        let catalog = FakeCatalog::default().with_table(
            "ProjectSet",
            &["ID", "IsActiveEntity", "status"],
        );
        let plan = migration_plan(&catalog, &SQLITE, &schema).await.unwrap();
        let update = plan.iter().position(|s| s.starts_with("UPDATE \"ProjectSet\"")).expect("UPDATE");
        let drop = plan.iter().position(|s| s.contains("DROP COLUMN \"status\"")).expect("DROP");
        assert!(update < drop, "Uebersetzung muss vor dem Entfernen laufen");
        assert!(plan[update].contains("WHEN 'open' THEN CAST('00000000-0000-0000-0000-000000000001' AS"));
    }

    #[tokio::test]
    async fn fresh_legacy_free_table_has_no_drop() {
        use data_store::schema::EntitySet;
        let set = EntitySet::new("ProjectSet", "Project").drafts();
        let schema = Schema::builder().set(set).build().unwrap();
        let catalog = FakeCatalog::default().with_table("ProjectSet", &["ID"]);
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
}

#[cfg(test)]
mod add_column_default_tests {
    use super::*;

    #[test]
    fn added_column_carries_default_when_given() {
        let sql = add_column_sql("T", "Owner_ID", "TEXT", Some("'abc'"));
        assert_eq!(sql, "ALTER TABLE \"T\" ADD COLUMN \"Owner_ID\" TEXT DEFAULT 'abc'");
    }

    #[test]
    fn added_column_without_default_stays_plain() {
        let sql = add_column_sql("T", "Note", "TEXT", None);
        assert_eq!(sql, "ALTER TABLE \"T\" ADD COLUMN \"Note\" TEXT");
    }
}
