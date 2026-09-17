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

/// `CREATE TABLE` fuer ein Entity-Set.
///
/// Fremdschluessel werden **nur fuer Kompositionen** erzeugt, und zwar
/// inklusive `IsActiveEntity`: damit kann ein aktiver Parent strukturell kein
/// Draft-Kind haben. Lose Assoziationen bekommen keinen Constraint - sonst
/// muesste ein Draft-Auftrag auf einen Draft-Kunden zeigen.
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

    if let Some(parent) = set.composition_parent() {
        let target = schema
            .set(&parent.target_set)
            .map_err(|e| StoreError::Metadata(format!("DDL: {e}")))?;
        let fk_column = parent.foreign_key.column();
        if set.draft_enabled && target.draft_enabled {
            parts.push(format!(
                "FOREIGN KEY ({}, {}) REFERENCES {} ({}, {}) ON DELETE CASCADE",
                quote(fk_column),
                quote(IS_ACTIVE_COLUMN),
                quote(&target.name),
                quote(ID_COLUMN),
                quote(IS_ACTIVE_COLUMN)
            ));
        } else {
            parts.push(format!(
                "FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE CASCADE",
                quote(fk_column),
                quote(&target.name),
                quote(ID_COLUMN)
            ));
        }
    }

    Ok(format!(
        "CREATE TABLE IF NOT EXISTS {} (\n  {}\n)",
        quote(&set.name),
        parts.join(",\n  ")
    ))
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

/// Nachtraeglich ergaenzte Spalten sind immer NULL-bar: fuer NOT NULL
/// braeuchte es einen Default, und den kann das Schema nicht nachliefern,
/// ohne ueber die bestehenden Zeilen zu entscheiden.
pub fn add_column_sql(table: &str, column: &str, column_type: &str) -> String {
    format!(
        "ALTER TABLE {} ADD COLUMN {} {}",
        quote(table),
        quote(column),
        column_type
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
                plan.push(add_column_sql(&set.name, &name, &ty));
            }
        } else {
            plan.push(create_table_sql(dialect, schema, set)?);
        }

        plan.extend(index_statements(set));
    }
    Ok(plan)
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
