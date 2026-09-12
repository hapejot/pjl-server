//! Der Extender (YamlStore verdrahtet) gegen die gemeinsame Verhaltenssuite.
//! Beweist, dass das Hook-System das observable Verhalten des Stores nicht aendert.

use std::fs;
use std::sync::Arc;

use data_store::conformance::{self, Harness};
use data_store::DataStore;
use fiori_server::hook::{DefaultHook, Extender};
use odata_yaml_store::YamlStore;
use tempfile::TempDir;

struct ExtenderHarness {
    dir: TempDir,
    store: Extender,
}

fn open(path: &std::path::Path) -> YamlStore {
    let store = YamlStore::open(path).expect("store");
    store.set_navigations(conformance::schema().to_navigations());
    store
}

impl Harness for ExtenderHarness {
    type Store = Extender;

    fn create() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let ystore = Arc::new(open(dir.path()));
        let store = Extender::build(ystore, Arc::new(DefaultHook));
        Self { dir, store }
    }

    fn store(&self) -> &Self::Store {
        &self.store
    }

    fn count_state(&self, set_name: &str, is_active: bool) -> usize {
        let path = self
            .dir
            .path()
            .join(set_name)
            .join(if is_active { "active" } else { "draft" });
        fs::read_dir(path)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("yaml"))
                    .count()
            })
            .unwrap_or(0)
    }

    fn reopen(&self) -> Option<Self::Store> {
        let ystore = Arc::new(open(self.dir.path()));
        Some(Extender::build(ystore, Arc::new(DefaultHook)))
    }
}

data_store::conformance_tests!(ExtenderHarness);
