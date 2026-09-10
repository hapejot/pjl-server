//! Der YAML-Store gegen die gemeinsame Verhaltenssuite.

use std::fs;

use data_store::conformance::{self, Harness};
use data_store::DataStore;
use odata_yaml_store::YamlStore;
use tempfile::TempDir;

struct YamlHarness {
    dir: TempDir,
    store: YamlStore,
}

fn open(path: &std::path::Path) -> YamlStore {
    let store = YamlStore::open(path).expect("store");
    store.set_navigations(conformance::schema().to_navigations());
    store
}

impl Harness for YamlHarness {
    type Store = YamlStore;

    fn create() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let store = open(dir.path());
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
        Some(open(self.dir.path()))
    }
}

data_store::conformance_tests!(YamlHarness);
