//! Systemkatalog von PostgreSQL.
//!
//! Postgres beantwortet die Frage nach vorhandenen Tabellen und Spalten ueber
//! `information_schema`. Die Abfragen grenzen auf das aktuelle Schema ein
//! (`current_schema()`), damit eine Datenbank mit mehreren isolierten
//! Schemata sich nicht selbst ins Gehege kommt.

use crate::sqltrace;
use std::collections::HashSet;

use data_store::StoreError;
use relational_core::catalog::ExistingForeignKey;
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

    async fn foreign_keys(&self, table: &str) -> Result<Vec<ExistingForeignKey>, StoreError> {
        let rows = sqltrace::query(
            self.client,
            "SELECT c.conname, \
                    array_agg(a.attname::text ORDER BY a.attname::text), \
                    r.relname::text \
             FROM pg_constraint c \
             JOIN pg_class t ON t.oid = c.conrelid \
             JOIN pg_namespace n ON n.oid = t.relnamespace \
             JOIN pg_class r ON r.oid = c.confrelid \
             JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = ANY (c.conkey) \
             WHERE c.contype = 'f' AND n.nspname = current_schema() AND t.relname = $1 \
             GROUP BY c.conname, r.relname",
            &[&table],
        )
        .await
        .map_err(catalog_err)?;
        Ok(rows
            .iter()
            .map(|r| ExistingForeignKey {
                name: r.get::<_, String>(0),
                columns: r.get::<_, Vec<String>>(1),
                target_table: r.get::<_, String>(2),
            })
            .collect())
    }
}
