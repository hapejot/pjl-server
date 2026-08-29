use std::sync::Arc;

use odata_yaml_store::DraftDataStore;
use spec::data_dictionary::DataDictonary;

pub mod batch;
pub mod flp;
pub mod srv;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn DraftDataStore>,
    pub ddic: Arc<DataDictonary>,
}
