//! DDL-Erzeugung und Migration.
//!
//! Das Schema ist die Wahrheit; die Datenbank wird darauf gebracht. Additive
//! Aenderungen laufen automatisch, destruktive brauchen einen expliziten
//! Aufruf - ein Serverstart soll keine Spalten verlieren.

use std::collections::HashSet;

use data_store::schema::{
    Column, ColumnRole, DataType, EntitySet, Schema, ID_COLUMN, IS_ACTIVE_COLUMN,
};
use data_store::StoreError;
use rusqlite::Connection;

use crate::dialect::Dialect;
use tracing::{debug, info};

/// Spalte mit den Draft-Verwaltungsdaten (FK auf [`DRAFT_ADMIN_TABLE`]).
pub const DRAFT_ADMIN_COLUMN: &str = "DraftAdministrativeData_ID";
/// Validierungsmeldungen des Draft-Flows, als JSON abgelegt.
pub const DRAFT_MESSAGES_COLUMN: &str = "DraftMessages";
/// Tabelle der Draft-Verwaltungsdaten.
pub const DRAFT_ADMIN_TABLE: &str = "DraftAdministrativeData";
/// Ablage der angewandten Schema-Revision.
pub const REVISION_TABLE: &str = "_schema_revision";

// pub fn sql_err(e: rusqlite::Error) -> StoreError {
//     StoreError::Conversion(format!("DDL: {e}"))
// }

/// Erzeugt einen `StoreError`, der Modul und Zeilennummer der Fehlerstelle
/// enthaelt.
///
/// Die Funktion `sql_err` kann das nicht - sie ortet sich immer an ihrer
/// eigenen Definition, nicht am Aufruf. Der Makro bindet `module_path!()` und
/// `line!()` an der Aufrufstelle ein. `store.rs` nutzt weiterhin `sql_err`.
macro_rules! ddl_err {
    ($variant:ident, $e:expr) => {
        StoreError::$variant(format!("DDL ({}:{}): {}", module_path!(), line!(), $e))
    };
}

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
            .map_err(|e| ddl_err!(Metadata, e))?;
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

/// DDL der Draft-Verwaltungstabelle.
///
/// Sie steht nicht im Schema, weil sie kein Entity-Set ist - der Typaufbau
/// muss trotzdem ueber den Dialekt laufen: `DraftIsCreatedByMe` ist ein
/// Wahrheitswert, und dessen Speicherung unterscheidet sich.
fn draft_admin_ddl(dialect: &dyn Dialect) -> String {
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

/// Bereits vorhandene Spalten einer Tabelle.
fn existing_columns(conn: &Connection, table: &str) -> Result<HashSet<String>, StoreError> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({})", quote(table)))
        .map_err(|e| ddl_err!(Conversion, e))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| ddl_err!(Conversion, e))?;
    let mut columns = HashSet::new();
    for row in rows {
        columns.insert(row.map_err(|e| ddl_err!(Conversion, e))?);
    }
    Ok(columns)
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, StoreError> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .map_err(|e| ddl_err!(Conversion, e))?;
    Ok(count > 0)
}

/// Bringt die Datenbank auf den Stand des Schemas.
///
/// Additiv: fehlende Tabellen anlegen, fehlende Spalten per
/// `ALTER TABLE ADD COLUMN` ergaenzen. Nachtraeglich ergaenzte Spalten sind
/// immer NULL-bar, weil SQLite fuer NOT NULL einen Default verlangt.
/// Typaenderungen und Spalten-Drops werden **nicht** automatisch ausgefuehrt -
/// sie brauchen einen Tabellen-Rebuild und damit eine bewusste Entscheidung.
pub fn ensure_schema(
    conn: &Connection,
    dialect: &dyn Dialect,
    schema: &Schema,
) -> Result<(), StoreError> {
    conn.execute_batch(&draft_admin_ddl(dialect))
        .map_err(|e| ddl_err!(Conversion, e))?;
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS {} (revision {} NOT NULL)",
        quote(REVISION_TABLE),
        dialect.column_type(&DataType::Int64)
    ))
    .map_err(|e| ddl_err!(Conversion, e))?;

    for set in schema.creation_order().map_err(|e| ddl_err!(Metadata, e))? {
        if table_exists(conn, &set.name)? {
            let existing = existing_columns(conn, &set.name)?;
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
                conn.execute_batch(&format!(
                    "ALTER TABLE {} ADD COLUMN {} {}",
                    quote(&set.name),
                    quote(&name),
                    ty
                ))
                .map_err(|e| ddl_err!(Conversion, e))?;
            }
        } else {
            let ddl = create_table_sql(dialect, schema, set)?;
            debug!("DDL: {ddl}");
            conn.execute_batch(&ddl)
                .map_err(|e| ddl_err!(Conversion, e))?;
        }

        for stmt in index_statements(set) {
            conn.execute_batch(&stmt)
                .map_err(|e| ddl_err!(Conversion, e))?;
        }
    }
    Ok(())
}

/// Meldet Abweichungen, die `ensure_schema` bewusst nicht selbst repariert.
pub fn pending_destructive_changes(
    conn: &Connection,
    dialect: &dyn Dialect,
    schema: &Schema,
) -> Result<Vec<String>, StoreError> {
    let mut findings = vec![];
    for set in schema.sets() {
        if !table_exists(conn, &set.name)? {
            continue;
        }
        let existing = existing_columns(conn, &set.name)?;
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

/// Liest die zuletzt angewandte Revision.
pub fn applied_revision(conn: &Connection) -> Option<u64> {
    conn.query_row(
        &format!("SELECT revision FROM {} LIMIT 1", quote(REVISION_TABLE)),
        [],
        |row| row.get::<_, i64>(0),
    )
    .ok()
    .map(|v| v as u64)
}

pub fn set_applied_revision(conn: &Connection, revision: u64) -> Result<(), StoreError> {
    conn.execute(&format!("DELETE FROM {}", quote(REVISION_TABLE)), [])
        .map_err(|e| ddl_err!(Conversion, e))?;
    conn.execute(
        &format!(
            "INSERT INTO {} (revision) VALUES (?1)",
            quote(REVISION_TABLE)
        ),
        [revision as i64],
    )
    .map_err(|e| ddl_err!(Conversion, e))?;
    Ok(())
}
