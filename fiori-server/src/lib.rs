use std::sync::Arc;

use odata_yaml_store::YamlStore;
use spec::data_dictionary::DataDictonary;

pub mod batch;
pub mod flp;
pub mod srv;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<YamlStore>,
    pub ddic: Arc<DataDictonary>,
}
