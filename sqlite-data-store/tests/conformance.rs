//! Der SQLite-Dokumentenstore gegen die gemeinsame Verhaltenssuite.

use data_store::conformance::{self, Harness};
use data_store::DataStore;
use sqlite_data_store::SqliteStore;
use tempfile::TempDir;

struct SqliteDocHarness {
    dir: TempDir,
    store: SqliteStore,
}

async fn open(path: &std::path::Path) -> SqliteStore {
    let store = SqliteStore::open(path).expect("store");
    store.set_navigations(conformance::schema().to_navigations());
    store
}

#[async_trait::async_trait]
#[async_trait::async_trait]
impl Harness for SqliteDocHarness {
    type Store = SqliteStore;

    async fn create() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let store = open(dir.path()).await;
        Self { dir, store }
    }

    async fn store(&self) -> &Self::Store {
        &self.store
    }

    async fn count_state(&self, set_name: &str, is_active: bool) -> usize {
        self.store.count_records(set_name, is_active)
    }

    async fn reopen(&self) -> Option<Self::Store> {
        Some(open(self.dir.path()).await)
    }
}

data_store::conformance_tests!(SqliteDocHarness);
