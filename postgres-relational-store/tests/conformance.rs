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
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use postgres_relational_store::PostgresStore;
use tokio_postgres::NoTls;

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

/// Pool, dessen Verbindungen alle auf demselben Testschema stehen.
///
/// Der `search_path` gehoert zur Verbindung, nicht zur Sitzung des Aufrufers -
/// bei einem Pool muss er deshalb in die Konfiguration, sonst landet die
/// zweite Verbindung wieder in `public`.
fn pool_for(schema_name: &str) -> Pool {
    let mut config: tokio_postgres::Config = url().parse().expect("Verbindungszeichenfolge");
    config.options(&format!("-c search_path=\"{schema_name}\""));
    let manager = Manager::from_config(
        config,
        NoTls,
        ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        },
    );
    Pool::builder(manager).max_size(8).build().expect("pool")
}

async fn create_schema(schema_name: &str) {
    let mut base: tokio_postgres::Config = url().parse().expect("Verbindungszeichenfolge");
    let (client, connection) = base.connect(NoTls).await.unwrap_or_else(|e| {
        panic!(
            "keine Verbindung zu {}: {e}\n\
             Laeuft die Datenbank? docker compose --profile dev up -d postgres",
            url()
        )
    });
    tokio::spawn(connection);
    client
        .batch_execute(&format!("CREATE SCHEMA IF NOT EXISTS \"{schema_name}\""))
        .await
        .expect("Testschema anlegen");
}

async fn open(schema_name: &str) -> PostgresStore {
    let source = Arc::new(StaticSchema::new(conformance::schema()));
    PostgresStore::from_pool(pool_for(schema_name), source)
        .await
        .expect("store")
}

#[async_trait::async_trait]
#[async_trait::async_trait]
impl Harness for PostgresHarness {
    type Store = PostgresStore;

    async fn create() -> Self {
        let schema_name = format!("test_{}", uuid::Uuid::new_v4().simple());
        create_schema(&schema_name).await;
        let store = open(&schema_name).await;
        Self { schema_name, store }
    }

    async fn store(&self) -> &Self::Store {
        &self.store
    }

    async fn count_state(&self, set_name: &str, is_active: bool) -> usize {
        self.store.count_state(set_name, is_active).await
    }

    /// Eine neue Verbindung auf dasselbe Schema - damit wird geprueft, dass
    /// die Daten in der Datenbank stehen und nicht im Prozess.
    async fn reopen(&self) -> Option<Self::Store> {
        Some(open(&self.schema_name).await)
    }
}

impl Drop for PostgresHarness {
    /// Aufraeumen ueber eine eigene Verbindung. `Drop` ist synchron, deshalb
    /// laeuft das Loeschen in einem abgesetzten Task - der Testlauf muss
    /// nicht darauf warten.
    fn drop(&mut self) {
        let schema_name = self.schema_name.clone();
        let url = url();
        tokio::spawn(async move {
            let Ok((client, connection)) = tokio_postgres::connect(&url, NoTls).await else {
                return;
            };
            tokio::spawn(connection);
            let _ = client
                .batch_execute(&format!("DROP SCHEMA IF EXISTS \"{schema_name}\" CASCADE"))
                .await;
        });
    }
}

data_store::conformance_tests!(PostgresHarness);
