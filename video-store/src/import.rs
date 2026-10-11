//! Ein-Schuss-Import: Postgres-Zeilen -> `DataStore::create_entity`.
//!
//! Zwei Schritte, getrennt damit der Teil ohne Datenbank testbar ist:
//! [`records_for_table`] wandelt Zeilen in Store-Datensaetze (reine Funktion),
//! [`import_table`] schreibt sie in Parent-vor-Kind-Reihenfolge.

use anyhow::{bail, Context};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use data_store::DataStore;
use serde_json::{json, Map, Value};
use tokio_postgres::Client;
use tracing::{error, info};
use uuid::Uuid;

use crate::introspect;
use crate::map::{KeyMode, Model, TableMap, KEY_NAMESPACE};

#[derive(Debug, Default)]
pub struct ImportReport {
    /// `(set, rows written)` in Schreibreihenfolge.
    pub written: Vec<(String, usize)>,
}

/// Importiert alle Tabellen des Modells in Parent-vor-Kind-Reihenfolge.
///
/// Der Store muss leer sein. Ein zweiter Lauf auf denselben Datenbestand
/// scheitert an den Primaerschluesseln; dann das Datenverzeichnis loeschen.
pub async fn import_all<S: DataStore>(
    client: &Client,
    store: &S,
    model: &Model,
) -> anyhow::Result<ImportReport> {
    info!("import all");
    let schema = model.schema()?;
    let order = schema
        .creation_order()
        .map_err(|e| anyhow::anyhow!("Schreibreihenfolge: {e}"))?;

    let mut report = ImportReport::default();
    for set in order {
        info!("import {}", set.name);
        let table = model
            .table_for_set(&set.name)
            .with_context(|| format!("kein Tabellenmapping fuer {}", set.name))?;
        let rows = introspect::read_rows(client, &table.table).await?;
        let count = write_rows(store, model, table, &rows).await?;
        report.written.push((table.set.clone(), count));
    }
    Ok(report)
}

/// Schreibt die Zeilen einer Tabelle. Der Aufrufer haelt die Reihenfolge ein.
pub async fn write_rows<S: DataStore>(
    store: &S,
    model: &Model,
    table: &TableMap,
    rows: &[Map<String, Value>],
) -> anyhow::Result<usize> {
    let records = records_for_table(model, table, rows)?;
    for record in &records {
        match store
            .create_entity(&table.set, record)
            .await
            .with_context(|| format!("{}: Zeile {} wurde abgelehnt", table.table, record["ID"]))
        {
            Ok(_) => {}
            Err(e) => error!("create entity: {e}"),
        }
    }
    info!("{}: {} rows", table.table, records.len());
    Ok(records.len())
}

/// Wandelt die Zeilen einer Tabelle in Store-Datensaetze (aktive Eintraege).
pub fn records_for_table(
    model: &Model,
    table: &TableMap,
    rows: &[Map<String, Value>],
) -> anyhow::Result<Vec<Value>> {
    let mut seen: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(rows.len());

    for row in rows {
        let mut record = Map::new();
        record.insert("ID".into(), Value::String(identity(table, row, &mut seen)?));
        record.insert("IsActiveEntity".into(), Value::Bool(true));

        for atom in &table.atoms {
            let raw = match &atom.source {
                Some(column) => row.get(column).cloned().unwrap_or(Value::Null),
                None => derived_name(table, row),
            };
            let value = if atom.binary {
                hex_to_base64(raw)?
            } else if atom.edm == "Edm.String" {
                as_text(raw)
            } else {
                raw
            };
            record.insert(atom.name.clone(), value);
        }

        for r in &table.refs {
            let raw = row.get(&r.source).cloned().unwrap_or(Value::Null);
            let value = if raw.is_null() {
                Value::Null
            } else {
                let target = model
                    .tables
                    .get(&r.target)
                    .with_context(|| format!("Ziel '{}' fehlt", r.target))?;
                Value::String(target_id(target, &raw)?)
            };
            record.insert(r.store_column.clone(), value);
        }

        out.push(Value::Object(record));
    }
    Ok(out)
}

/// `name` ohne Namensspalte: Schluessel- bzw. Fremdschluesselwerte, mit ` / ` verbunden.
fn derived_name(table: &TableMap, row: &Map<String, Value>) -> Value {
    let columns = table.derived_name_columns();
    if columns.is_empty() {
        return Value::String(Value::Object(row.clone()).to_string());
    }
    let parts: Vec<String> = columns
        .iter()
        .map(|c| match row.get(c) {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
        })
        .collect();
    Value::String(parts.join(" / "))
}

/// Store-ID einer Quellzeile.
fn identity(
    table: &TableMap,
    row: &Map<String, Value>,
    seen: &mut std::collections::HashSet<Uuid>,
) -> anyhow::Result<String> {
    match &table.key {
        KeyMode::Uuid { column } => {
            let raw = row
                .get(column)
                .and_then(Value::as_str)
                .with_context(|| format!("{}: PK '{column}' fehlt", table.table))?;
            Uuid::parse_str(raw).with_context(|| format!("{}: ungueltige UUID", table.table))?;
            Ok(raw.to_string())
        }
        KeyMode::Legacy { column } => {
            let raw = row
                .get(column)
                .with_context(|| format!("{}: PK '{column}' fehlt", table.table))?;
            Ok(derive(&table.table, &key_text(raw)?).to_string())
        }
        KeyMode::Synth { columns } => {
            let text = if columns.is_empty() {
                serde_json::to_string(row)?
            } else {
                columns
                    .iter()
                    .map(|c| {
                        row.get(c)
                            .map(key_text)
                            .unwrap_or_else(|| Ok(String::new()))
                    })
                    .collect::<anyhow::Result<Vec<_>>>()?
                    .join("\u{1f}")
            };
            // Vollstaendig gleiche Zeilen (nur ohne PK moeglich) bekommen einen Zaehler.
            let mut candidate = text.clone();
            let mut n = 1;
            let mut id = derive(&table.table, &candidate);
            while seen.contains(&id) {
                candidate = format!("{text}#{n}");
                id = derive(&table.table, &candidate);
                n += 1;
            }
            seen.insert(id);
            Ok(id.to_string())
        }
    }
}

/// Store-ID fuer einen Fremdschluessel-Wert, passend zum Zielmodell.
fn target_id(target: &TableMap, raw: &Value) -> anyhow::Result<String> {
    match &target.key {
        KeyMode::Uuid { .. } => Ok(raw
            .as_str()
            .with_context(|| format!("{}: UUID-Wert erwartet", target.table))?
            .to_string()),
        KeyMode::Legacy { .. } => Ok(derive(&target.table, &key_text(raw)?).to_string()),
        KeyMode::Synth { .. } => bail!("'{}' hat keinen referenzierbaren Schluessel", target.table),
    }
}

fn derive(table: &str, key: &str) -> Uuid {
    Uuid::new_v5(&KEY_NAMESPACE, format!("{table}\u{1f}{key}").as_bytes())
}

fn key_text(v: &Value) -> anyhow::Result<String> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        other => bail!("Schluesselwert nicht als Text darstellbar: {other}"),
    }
}

/// `Edm.String`-Felder halten immer Text. jsonb-Werte und Zahlen (z. B. ein
/// bigint-Schluessel als `legacy_id`) werden als JSON-Text abgelegt.
fn as_text(raw: Value) -> Value {
    match raw {
        Value::Null | Value::String(_) => raw,
        other => Value::String(other.to_string()),
    }
}

/// Postgres `bytea` kommt aus `row_to_json` als `"\\x<hex>"`. Der Store
/// erwartet Text, daher Base64.
fn hex_to_base64(raw: Value) -> anyhow::Result<Value> {
    let Value::String(s) = &raw else {
        return Ok(raw);
    };
    let Some(hex) = s.strip_prefix("\\x") else {
        return Ok(raw);
    };
    if hex.len() % 2 != 0 {
        bail!("ungerade Hex-Laenge in bytea");
    }
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .context("ungueltiger Hex-Wert in bytea")?;
    Ok(json!(STANDARD.encode(bytes)))
}
