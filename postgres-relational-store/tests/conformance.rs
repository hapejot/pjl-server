//! Der Postgres-Store gegen die gemeinsame Verhaltenssuite.
//!
//! Dieselben Erwartungen wie fuer die uebrigen Backends. Weicht Postgres ab,
//! ist das ein Fehler im Backend - nicht ein Grund, die Suite zu lockern.
//!
//! Voraussetzung ist eine laufende Datenbank:
//!
//! ```text
//! docker compose --profile dev up -d postgres
//! cargo test -p postgres-relational-store --features docker-tests
//! ```
#![cfg(feature = "docker-tests")]

use std::sync::Arc;

use data_store::conformance::{self, Harness};
use data_store::StaticSchema;
use postgres::{Client, NoTls};
use postgres_relational_store::PostgresStore;

/// Verbindungszeichenfolge; `PJL_TEST_POSTGRES` sticht die Vorgabe aus
/// `compose.yaml`.
fn url() -> String {
    std::env::var("PJL_TEST_POSTGRES")
        .unwrap_or_else(|_| "postgres://pjl:pjl@localhost:5433/pjl".into())
}

/// Jeder Test bekommt ein eigenes Postgres-Schema.
///
/// Eine Datenbank je Test waere langsam, eine geteilte Datenbank waere nicht
/// isoliert. Ein Schema pro Test kostet fast nichts und haelt die Tabellen
/// auseinander - der Katalog grenzt ueber `current_schema()` ohnehin darauf
/// ein.
struct PostgresHarness {
    schema_name: String,
    store: PostgresStore,
}

fn connect_to(schema_name: &str) -> Client {
    let mut client = Client::connect(&url(), NoTls).unwrap_or_else(|e| {
        panic!(
            "keine Verbindung zu {}: {e}\n\
             Laeuft die Datenbank? docker compose --profile dev up -d postgres",
            url()
        )
    });
    client
        .batch_execute(&format!(
            "CREATE SCHEMA IF NOT EXISTS \"{schema_name}\"; \
             SET search_path TO \"{schema_name}\""
        ))
        .expect("Testschema anlegen");
    client
}

fn open(schema_name: &str) -> PostgresStore {
    let source = Arc::new(StaticSchema::new(conformance::schema()));
    PostgresStore::from_client(connect_to(schema_name), source).expect("store")
}

impl Harness for PostgresHarness {
    type Store = PostgresStore;

    fn create() -> Self {
        let schema_name = format!("test_{}", uuid::Uuid::new_v4().simple());
        let store = open(&schema_name);
        Self { schema_name, store }
    }

    fn store(&self) -> &Self::Store {
        &self.store
    }

    fn count_state(&self, set_name: &str, is_active: bool) -> usize {
        self.store.count_state(set_name, is_active)
    }

    /// Eine neue Verbindung auf dasselbe Schema - damit wird geprueft, dass
    /// die Daten in der Datenbank stehen und nicht im Prozess.
    fn reopen(&self) -> Option<Self::Store> {
        Some(open(&self.schema_name))
    }
}

impl Drop for PostgresHarness {
    fn drop(&mut self) {
        // Aufraeumen ueber eine eigene Verbindung: die des Stores ist noch
        // belegt, und ein DROP auf dem eigenen search_path scheitert daran.
        if let Ok(mut client) = Client::connect(&url(), NoTls) {
            let _ = client.batch_execute(&format!(
                "DROP SCHEMA IF EXISTS \"{}\" CASCADE",
                self.schema_name
            ));
        }
    }
}

data_store::conformance_tests!(PostgresHarness);
