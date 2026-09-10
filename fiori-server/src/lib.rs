use std::sync::{Arc, RwLock};

use odata_yaml_store::DraftDataStore;
use spec::data_dictionary::DataDictonary;

pub mod batch;
pub mod ddic_loader;
pub mod flp;
pub mod hook;
pub mod srv;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn DraftDataStore>,
    ddic: Arc<RwLock<Arc<DataDictonary>>>,
}

impl AppState {
    pub fn new(store: Arc<dyn DraftDataStore>) -> Self {
        let mut ddic = DataDictonary::new();
        ddic.activate();
        Self {
            store,
            ddic: Arc::new(RwLock::new(Arc::new(ddic))),
        }
    }

    /// Snapshot des aktuellen Dictionary - bleibt fuer die Dauer eines Requests stabil.
    pub fn ddic(&self) -> Arc<DataDictonary> {
        self.ddic.read().expect("ddic lock").clone()
    }

    /// Dictionary komplett neu aufbauen; Lookups entstehen dabei erneut aus den Relations.
    pub fn reload(&self) -> Arc<DataDictonary> {
        let mut ddic = DataDictonary::new();
        ddic_loader::load_ddic_from_store(&mut ddic, self.store.as_ref());
        ddic.activate();
        self.store
            .set_navigations(ddic_loader::navigations_from_ddic(&ddic));

        let ddic = Arc::new(ddic);
        *self.ddic.write().expect("ddic lock") = ddic.clone();
        ddic
    }
}
