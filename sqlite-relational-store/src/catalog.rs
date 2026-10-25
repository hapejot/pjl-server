//! Systemkatalog von SQLite.
//!
//! Der Katalog haelt die Verbindung ueber ihren Mutex, nicht als nackte
//! Referenz. `rusqlite::Connection` ist zwar `Send`, aber nicht `Sync` - eine
//! `&Connection` koennte die Migration nicht ueber einen `await`-Punkt
//! hinweg festhalten. Gesperrt wird innerhalb der Methode und dort auch
//! wieder freigegeben; zwischen Sperre und Rueckgabe liegt kein `await`.

use std::collections::HashSet;
use std::sync::Mutex;

use data_store::StoreError;
use relational_core::ddl::quote;
use relational_core::catalog::ExistingForeignKey;
use relational_core::Catalog;
use rusqlite::Connection;

pub struct SqliteCatalog<'a> {
    conn: &'a Mutex<Connection>,
}

impl<'a> SqliteCatalog<'a> {
    pub fn new(conn: &'a Mutex<Connection>) -> Self {
        Self { conn }
    }
}

fn catalog_err(e: impl std::fmt::Display) -> StoreError {
    StoreError::Conversion(format!("Katalog: {e}"))
}

#[async_trait::async_trait]
impl Catalog for SqliteCatalog<'_> {
    async fn table_exists(&self, table: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock().expect("connection lock");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .map_err(catalog_err)?;
        Ok(count > 0)
    }

    async fn existing_columns(&self, table: &str) -> Result<HashSet<String>, StoreError> {
        // PRAGMA nimmt den Tabellennamen nicht als Parameter entgegen. Der
        // Name stammt aus dem Schema, nicht aus einer Anfrage, und wird
        // zusaetzlich gequotet.
        let conn = self.conn.lock().expect("connection lock");
        let mut stmt = conn
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

    /// SQLite kann Constraints nicht nachtraeglich aendern: kein Abgleich.
    async fn foreign_keys(&self, _table: &str) -> Result<Vec<ExistingForeignKey>, StoreError> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sqlite_catalog_reads_its_own_tables() {
        let conn = Mutex::new(Connection::open_in_memory().unwrap());
        conn.lock()
            .unwrap()
            .execute_batch(r#"CREATE TABLE "OrderSet" ("ID" TEXT, "Amount" INTEGER)"#)
            .unwrap();
        let catalog = SqliteCatalog::new(&conn);
        assert!(catalog.table_exists("OrderSet").await.unwrap());
        assert!(!catalog.table_exists("Nope").await.unwrap());
        let columns = catalog.existing_columns("OrderSet").await.unwrap();
        assert!(columns.contains("ID") && columns.contains("Amount"));
        assert!(catalog.existing_columns("Nope").await.unwrap().is_empty());
    }
}
