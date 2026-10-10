//! Liest den Katalog der Quelldatenbank (Postgres) und die Zeilen der Tabellen.
//!
//! Bewusst kein Parsen des `pg_dump`: der Katalog ist die Wahrheit, und der
//! Generator bleibt wiederholbar, falls das Produktivschema sich aendert.
//! Alle Abfragen sind lesend.

use std::collections::BTreeMap;

use anyhow::{Context, bail};
use serde_json::{Map, Value};
use tokio_postgres::{Client, NoTls};

#[derive(Debug, Clone)]
pub struct PgColumn {
    pub name: String,
    /// `udt_name` aus `information_schema` (z. B. `text`, `int4`, `uuid`,
    /// `vector`, `agent_status`).
    pub udt: String,
    pub nullable: bool,
    pub max_length: Option<u32>,
    pub precision: Option<u32>,
    pub scale: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct PgTable {
    pub name: String,
    /// Spalten in Katalogreihenfolge.
    pub columns: Vec<PgColumn>,
    /// Primaerschluessel in Constraint-Reihenfolge; leer = kein PK.
    pub pk: Vec<String>,
}

impl PgTable {
    pub fn column(&self, name: &str) -> Option<&PgColumn> {
        self.columns.iter().find(|c| c.name == name)
    }

    pub fn position(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }
}

/// Fremdschluessel, einspaltig. `on_delete` ist der `confdeltype` aus
/// `pg_constraint`: `c` = CASCADE, `n` = SET NULL, `a` = NO ACTION, ...
#[derive(Debug, Clone)]
pub struct PgForeignKey {
    pub child: String,
    pub child_col: String,
    pub parent: String,
    pub parent_col: String,
    pub on_delete: char,
}

#[derive(Debug, Clone, Default)]
pub struct PgCatalog {
    pub tables: BTreeMap<String, PgTable>,
    pub foreign_keys: Vec<PgForeignKey>,
}

pub async fn connect(url: &str) -> anyhow::Result<Client> {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .context("cannot connect to postgres")?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::error!("postgres connection: {e}");
        }
    });
    Ok(client)
}

/// Katalog des `public`-Schemas: Basistabellen (keine Views), Spalten, PKs, FKs.
pub async fn read_catalog(client: &Client) -> anyhow::Result<PgCatalog> {
    let mut catalog = PgCatalog::default();

    let rows = client
        .query(
            "SELECT c.table_name, c.column_name, c.udt_name, c.is_nullable,
                    c.character_maximum_length::int4, c.numeric_precision::int4,
                    c.numeric_scale::int4
             FROM information_schema.columns c
             WHERE c.table_schema = 'public'
               AND c.table_name IN (SELECT table_name FROM information_schema.tables
                                    WHERE table_schema = 'public'
                                      AND table_type = 'BASE TABLE')
             ORDER BY c.table_name, c.ordinal_position",
            &[],
        )
        .await
        .context("reading columns")?;
    for row in rows {
        let table: String = row.get(0);
        let column = PgColumn {
            name: row.get(1),
            udt: row.get(2),
            nullable: row.get::<_, String>(3) == "YES",
            max_length: row.get::<_, Option<i32>>(4).map(|v| v as u32),
            precision: row.get::<_, Option<i32>>(5).map(|v| v as u32),
            scale: row.get::<_, Option<i32>>(6).map(|v| v as u32),
        };
        catalog
            .tables
            .entry(table.clone())
            .or_insert_with(|| PgTable {
                name: table,
                columns: Vec::new(),
                pk: Vec::new(),
            })
            .columns
            .push(column);
    }

    // contype: p = primary key, f = foreign key
    let rows = client
        .query(
            "SELECT src.relname::text,
                    con.contype::text,
                    tgt.relname::text,
                    ARRAY(SELECT a.attname::text
                          FROM unnest(con.conkey) WITH ORDINALITY AS k(n, o)
                          JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.n
                          ORDER BY k.o),
                    ARRAY(SELECT a.attname::text
                          FROM unnest(con.confkey) WITH ORDINALITY AS k(n, o)
                          JOIN pg_attribute a ON a.attrelid = con.confrelid AND a.attnum = k.n
                          ORDER BY k.o),
                    con.confdeltype::text
             FROM pg_constraint con
             JOIN pg_class src ON src.oid = con.conrelid
             JOIN pg_namespace n ON n.oid = src.relnamespace
             LEFT JOIN pg_class tgt ON tgt.oid = con.confrelid
             WHERE n.nspname = 'public' AND con.contype IN ('p', 'f')",
            &[],
        )
        .await
        .context("reading constraints")?;
    for row in rows {
        let table: String = row.get(0);
        let kind: String = row.get(1);
        let src_cols: Vec<String> = row.get(3);
        match kind.as_str() {
            "p" => {
                if let Some(t) = catalog.tables.get_mut(&table) {
                    t.pk = src_cols;
                }
            }
            "f" => {
                let parent: String = row.get(2);
                let tgt_cols: Vec<String> = row.get(4);
                if src_cols.len() != 1 || tgt_cols.len() != 1 {
                    bail!("mehrspaltiger Fremdschluessel auf '{table}' wird nicht unterstuetzt");
                }
                let on_delete = row
                    .get::<_, String>(5)
                    .chars()
                    .next()
                    .unwrap_or('a');
                catalog.foreign_keys.push(PgForeignKey {
                    child: table,
                    child_col: src_cols[0].clone(),
                    parent,
                    parent_col: tgt_cols[0].clone(),
                    on_delete,
                });
            }
            _ => {}
        }
    }

    Ok(catalog)
}

/// Alle Zeilen einer Tabelle als JSON-Objekte.
///
/// `row_to_json` uebernimmt die Typwandlung: uuid und Zeitstempel als Text,
/// numerische Werte als Zahlen, `vector` als Text `"[0.1,0.2,...]"`, enums
/// als Text, `bytea` als Hex-Text `"\\x..."`. Die Feinheiten behandelt
/// `import.rs`.
pub async fn read_rows(client: &Client, table: &str) -> anyhow::Result<Vec<Map<String, Value>>> {
    let sql = format!(
        "SELECT row_to_json(t)::text FROM public.{} t",
        quote_ident(table)
    );
    let rows = client
        .query(sql.as_str(), &[])
        .await
        .with_context(|| format!("reading rows of '{table}'"))?;
    rows.into_iter()
        .map(|row| {
            let text: String = row.get(0);
            match serde_json::from_str::<Value>(&text)? {
                Value::Object(map) => Ok(map),
                other => bail!("unerwartete Zeile in '{table}': {other}"),
            }
        })
        .collect()
}

pub fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}
