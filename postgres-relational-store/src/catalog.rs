//! Systemkatalog von PostgreSQL.
//!
//! Postgres beantwortet die Frage nach vorhandenen Tabellen und Spalten ueber
//! `information_schema`. Der Suchpfad bleibt dabei bewusst unerwaehnt: die
//! Abfragen grenzen auf das aktuelle Schema ein (`current_schema()`), damit
//! eine Testdatenbank mit mehreren isolierten Schemata sich nicht selbst ins
//! Gehege kommt.

use std::cell::RefCell;
use std::collections::HashSet;

use data_store::StoreError;
use postgres::GenericClient;
use relational_core::Catalog;

/// Der Katalog liest, das Trait gibt aber nur `&self` her - der Treiber
/// verlangt fuer jede Abfrage `&mut`. Die [`RefCell`] ueberbrueckt das.
pub struct PostgresCatalog<'a, C: GenericClient> {
    client: RefCell<&'a mut C>,
}

impl<'a, C: GenericClient> PostgresCatalog<'a, C> {
    pub fn new(client: &'a mut C) -> Self {
        Self {
            client: RefCell::new(client),
        }
    }
}

fn catalog_err(e: impl std::fmt::Display) -> StoreError {
    StoreError::Conversion(format!("Katalog: {e}"))
}

impl<C: GenericClient> Catalog for PostgresCatalog<'_, C> {
    fn table_exists(&self, table: &str) -> Result<bool, StoreError> {
        let row = self
            .client
            .borrow_mut()
            .query_one(
                "SELECT COUNT(*) FROM information_schema.tables \
                 WHERE table_schema = current_schema() AND table_name = $1",
                &[&table],
            )
            .map_err(catalog_err)?;
        let count: i64 = row.get(0);
        Ok(count > 0)
    }

    fn existing_columns(&self, table: &str) -> Result<HashSet<String>, StoreError> {
        let rows = self
            .client
            .borrow_mut()
            .query(
                "SELECT column_name FROM information_schema.columns \
                 WHERE table_schema = current_schema() AND table_name = $1",
                &[&table],
            )
            .map_err(catalog_err)?;
        Ok(rows.iter().map(|r| r.get::<_, String>(0)).collect())
    }
}
