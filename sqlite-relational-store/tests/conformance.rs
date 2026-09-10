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

fn open(path: &std::path::Path) -> RelationalStore {
    let source = Arc::new(StaticSchema::new(conformance::schema()));
    RelationalStore::open(path, source).expect("store")
}

impl Harness for RelationalHarness {
    type Store = RelationalStore;

    fn create() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let store = open(dir.path());
        Self { dir, store }
    }

    fn store(&self) -> &Self::Store {
        &self.store
    }

    fn count_state(&self, set_name: &str, is_active: bool) -> usize {
        self.store.count_state(set_name, is_active)
    }

    fn reopen(&self) -> Option<Self::Store> {
        Some(open(self.dir.path()))
    }
}

data_store::conformance_tests!(RelationalHarness);
