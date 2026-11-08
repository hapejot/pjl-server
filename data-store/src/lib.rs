pub mod datastore;
pub mod error;
pub mod filter_eval;
pub mod key;
pub mod nav;
pub mod query;
pub mod schema;
pub mod sql_value;

#[cfg(any(test, feature = "conformance"))]
pub mod conformance;

#[cfg(any(test, feature = "debug-tools"))]
pub use datastore::DataStoreDebugExt;
pub use datastore::{
    DataRecord, DataStore, DraftAction, DraftAdministrativeData, DraftMessage, Navigation, WriteOp,
};
pub use error::StoreError;
pub use key::EntityKey;
pub use nav::{nav_links, NavLink, DRAFT_ADMIN_SET};
pub use schema::{
    Association, Cardinality, Column, ColumnRole, DataType, EntitySet, ForeignKeySide, Ownership,
    Schema, SchemaError, SchemaRevision, SchemaSource, StaticSchema,
};
pub use sql_value::{json_to_param, json_to_param_loose, param_to_json, SqlParam};

#[async_trait::async_trait]
pub trait DraftDataStore: DataStore {
    async fn draft_edit(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError>;
    async fn draft_activate(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError>;
    async fn draft_prepare(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError>;
}
