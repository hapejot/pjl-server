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

/// Ein Fremdschluessel, so wie die Datenbank ihn kennt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingForeignKey {
    pub name: String,
    /// Spalten der eigenen Tabelle, sortiert.
    pub columns: Vec<String>,
    pub target_table: String,
}

#[async_trait::async_trait]
pub trait Catalog: Sync {
    async fn table_exists(&self, table: &str) -> Result<bool, StoreError>;

    /// Namen der vorhandenen Spalten. Fuer eine unbekannte Tabelle leer.
    async fn existing_columns(&self, table: &str) -> Result<HashSet<String>, StoreError>;

    /// Fremdschluessel der Tabelle. Backends ohne Constraint-Aenderung
    /// (SQLite) liefern leer und bekommen dann keinen Abgleich.
    async fn foreign_keys(&self, table: &str) -> Result<Vec<ExistingForeignKey>, StoreError>;
}

/// Katalog ohne Datenbank.
///
/// Damit laesst sich die Migrationsplanung gegen einen beliebigen
/// Ausgangszustand pruefen, ohne ihn erst herstellen zu muessen.
#[derive(Default)]
pub struct FakeCatalog {
    pub tables: HashMap<String, HashSet<String>>,
    pub foreign_keys: HashMap<String, Vec<ExistingForeignKey>>,
}

impl FakeCatalog {
    pub fn with_table(mut self, table: &str, columns: &[&str]) -> Self {
        self.tables.insert(
            table.to_string(),
            columns.iter().map(|c| c.to_string()).collect(),
        );
        self
    }

    pub fn with_foreign_key(mut self, table: &str, name: &str, columns: &[&str], target: &str) -> Self {
        let mut columns: Vec<String> = columns.iter().map(|c| c.to_string()).collect();
        columns.sort();
        self.foreign_keys.entry(table.to_string()).or_default().push(ExistingForeignKey {
            name: name.to_string(),
            columns,
            target_table: target.to_string(),
        });
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

    async fn foreign_keys(&self, table: &str) -> Result<Vec<ExistingForeignKey>, StoreError> {
        Ok(self.foreign_keys.get(table).cloned().unwrap_or_default())
    }
}
