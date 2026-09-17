//! Lesender Zugriff auf den Systemkatalog der Datenbank.
//!
//! Die Migration muss wissen, was bereits existiert. *Wie* man das erfragt,
//! ist der am wenigsten portable Teil eines relationalen Stores: SQLite
//! antwortet ueber `PRAGMA` und `sqlite_master`, Postgres ueber
//! `information_schema`. Das Trait trennt diese Frage von der Migration
//! selbst - [`crate::ddl::migration_plan`] kommt dadurch ohne
//! Datenbankverbindung aus und ist ohne laufende Datenbank testbar.

use std::collections::{HashMap, HashSet};

use data_store::StoreError;

#[async_trait::async_trait]
pub trait Catalog: Sync {
    async fn table_exists(&self, table: &str) -> Result<bool, StoreError>;

    /// Namen der vorhandenen Spalten. Fuer eine unbekannte Tabelle leer.
    async fn existing_columns(&self, table: &str) -> Result<HashSet<String>, StoreError>;
}

/// Katalog ohne Datenbank.
///
/// Damit laesst sich die Migrationsplanung gegen einen beliebigen
/// Ausgangszustand pruefen, ohne ihn erst herstellen zu muessen.
#[derive(Default)]
pub struct FakeCatalog {
    pub tables: HashMap<String, HashSet<String>>,
}

impl FakeCatalog {
    pub fn with_table(mut self, table: &str, columns: &[&str]) -> Self {
        self.tables.insert(
            table.to_string(),
            columns.iter().map(|c| c.to_string()).collect(),
        );
        self
    }
}

#[async_trait::async_trait]
impl Catalog for FakeCatalog {
    async fn table_exists(&self, table: &str) -> Result<bool, StoreError> {
        Ok(self.tables.contains_key(table))
    }

    async fn existing_columns(&self, table: &str) -> Result<HashSet<String>, StoreError> {
        Ok(self.tables.get(table).cloned().unwrap_or_default())
    }
}
