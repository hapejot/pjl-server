//! Systemkatalog von PostgreSQL.
//!
//! Postgres beantwortet die Frage nach vorhandenen Tabellen und Spalten ueber
//! `information_schema`. Die Abfragen grenzen auf das aktuelle Schema ein
//! (`current_schema()`), damit eine Datenbank mit mehreren isolierten
//! Schemata sich nicht selbst ins Gehege kommt.

use crate::sqltrace;
use std::collections::HashSet;

use data_store::StoreError;
use relational_core::Catalog;
use deadpool_postgres::GenericClient;

pub struct PostgresCatalog<'a, C: GenericClient> {
    client: &'a C,
}

impl<'a, C: GenericClient> PostgresCatalog<'a, C> {
    pub fn new(client: &'a C) -> Self {
        Self { client }
    }
}

fn catalog_err(e: impl std::fmt::Display) -> StoreError {
    StoreError::Conversion(format!("Katalog: {e}"))
}

#[async_trait::async_trait]
impl<C: GenericClient + Sync> Catalog for PostgresCatalog<'_, C> {
    async fn table_exists(&self, table: &str) -> Result<bool, StoreError> {
        let row = sqltrace::query_one(self.client, "SELECT COUNT(*) FROM information_schema.tables \
                 WHERE table_schema = current_schema() AND table_name = $1",
                &[&table],
            )
            .await
            .map_err(catalog_err)?;
        let count: i64 = row.get(0);
        Ok(count > 0)
    }

    async fn existing_columns(&self, table: &str) -> Result<HashSet<String>, StoreError> {
        let rows = sqltrace::query(self.client, "SELECT column_name FROM information_schema.columns \
                 WHERE table_schema = current_schema() AND table_name = $1",
                &[&table],
            )
            .await
            .map_err(catalog_err)?;
        Ok(rows.iter().map(|r| r.get::<_, String>(0)).collect())
    }
}
