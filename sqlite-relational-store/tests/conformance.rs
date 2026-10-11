//! Der relationale Store gegen die gemeinsame Verhaltenssuite.
//!
//! Gleiche Erwartungen wie fuer die Dokumenten-Backends - nur die Ablage
//! unterscheidet sich.

use std::sync::Arc;

use data_store::conformance::{self, Harness};
use data_store::StaticSchema;
use sqlite_relational_store::RelationalStore;
use tempfile::TempDir;

struct RelationalHarness {
    dir: TempDir,
    store: RelationalStore,
}

async fn open(path: &std::path::Path) -> RelationalStore {
    let source = Arc::new(StaticSchema::new(conformance::schema()));
    RelationalStore::open(path, source).await.expect("store")
}

#[async_trait::async_trait]
#[async_trait::async_trait]
impl Harness for RelationalHarness {
    type Store = RelationalStore;

    async fn create() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let store = open(dir.path()).await;
        Self { dir, store }
    }

    async fn store(&self) -> &Self::Store {
        &self.store
    }

    async fn count_state(&self, set_name: &str, is_active: bool) -> usize {
        self.store.count_state(set_name, is_active)
    }

    async fn reopen(&self) -> Option<Self::Store> {
        // Betrieb nur im Speicher: der Stand muss vor dem Neuoeffnen auf die Platte.
        self.store.persist().expect("persist vor reopen");
        Some(open(self.dir.path()).await)
    }
}

data_store::conformance_tests!(RelationalHarness);
