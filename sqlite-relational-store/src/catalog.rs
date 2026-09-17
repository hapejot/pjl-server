//! Lesender Zugriff auf den Systemkatalog der Datenbank.
//!
//! Die Migration muss wissen, was bereits existiert. *Wie* man das erfragt,
//! ist der am wenigsten portable Teil des ganzen Stores: SQLite antwortet
//! ueber `PRAGMA` und `sqlite_master`, andere Datenbanken ueber
//! `information_schema`. Das Trait trennt diese Frage von der Migration
//! selbst - [`crate::ddl::migration_plan`] kommt dadurch ohne
//! Datenbankverbindung aus und ist ohne laufende Datenbank testbar.

use std::collections::HashSet;

use data_store::StoreError;
use rusqlite::Connection;

pub trait Catalog {
    fn table_exists(&self, table: &str) -> Result<bool, StoreError>;

    /// Namen der vorhandenen Spalten. Fuer eine unbekannte Tabelle leer.
    fn existing_columns(&self, table: &str) -> Result<HashSet<String>, StoreError>;
}

/// Katalog von SQLite.
pub struct SqliteCatalog<'a> {
    conn: &'a Connection,
}

impl<'a> SqliteCatalog<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }
}

fn catalog_err(e: impl std::fmt::Display) -> StoreError {
    StoreError::Conversion(format!("Katalog: {e}"))
}

impl Catalog for SqliteCatalog<'_> {
    fn table_exists(&self, table: &str) -> Result<bool, StoreError> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .map_err(catalog_err)?;
        Ok(count > 0)
    }

    fn existing_columns(&self, table: &str) -> Result<HashSet<String>, StoreError> {
        // PRAGMA nimmt den Tabellennamen nicht als Parameter entgegen. Der
        // Name stammt aus dem Schema, nicht aus einer Anfrage, und wird
        // zusaetzlich gequotet.
        let mut stmt = self
            .conn
            .prepare(&format!(
                "PRAGMA table_info({})",
                crate::ddl::quote(table)
            ))
            .map_err(catalog_err)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(catalog_err)?;
        let mut columns = HashSet::new();
        for row in rows {
            columns.insert(row.map_err(catalog_err)?);
        }
        Ok(columns)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Katalog ohne Datenbank, fuer Tests der Migrationsplanung.
    #[derive(Default)]
    pub struct FakeCatalog {
        pub tables: std::collections::HashMap<String, HashSet<String>>,
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

    impl Catalog for FakeCatalog {
        fn table_exists(&self, table: &str) -> Result<bool, StoreError> {
            Ok(self.tables.contains_key(table))
        }

        fn existing_columns(&self, table: &str) -> Result<HashSet<String>, StoreError> {
            Ok(self.tables.get(table).cloned().unwrap_or_default())
        }
    }

    #[test]
    fn sqlite_catalog_reads_its_own_tables() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(r#"CREATE TABLE "OrderSet" ("ID" TEXT, "Amount" INTEGER)"#)
            .unwrap();
        let catalog = SqliteCatalog::new(&conn);
        assert!(catalog.table_exists("OrderSet").unwrap());
        assert!(!catalog.table_exists("Nope").unwrap());
        let columns = catalog.existing_columns("OrderSet").unwrap();
        assert!(columns.contains("ID") && columns.contains("Amount"));
        assert!(catalog.existing_columns("Nope").unwrap().is_empty());
    }
}
