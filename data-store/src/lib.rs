pub mod datastore;
pub mod error;
pub mod filter_eval;
pub mod key;

pub use datastore::{
    DataRecord, DataStore, DraftAction, DraftAdministrativeData, DraftMessage, Navigation,
};
#[cfg(any(test, feature = "debug-tools"))]
pub use datastore::DataStoreDebugExt;
pub use error::StoreError;
pub use key::EntityKey;

pub trait DraftDataStore: DataStore {
    fn draft_edit(&self, set_name: &str, key: &EntityKey) -> Result<serde_json::Value, StoreError>;
    fn draft_activate(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError>;
    fn draft_prepare(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError>;
}
