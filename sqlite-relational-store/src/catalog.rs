//! Systemkatalog von SQLite.

use std::collections::HashSet;

use data_store::StoreError;
use relational_core::ddl::quote;
use relational_core::Catalog;
use rusqlite::Connection;

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
            .prepare(&format!("PRAGMA table_info({})", quote(table)))
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
mod tests {
    use super::*;

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
