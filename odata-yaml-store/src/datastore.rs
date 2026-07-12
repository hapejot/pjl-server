use crate::error::StoreError;
use crate::key::{EntityKey, ParentKey};
use odata::query::ODataQuery;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftAction {
    Edit,
    Activate,
    Prepare,
}

pub trait DataStore: Send + Sync {
    // -- Collections --
    fn get_collection(
        &self,
        set_name: &str,
        query: &ODataQuery,
        parent: Option<&ParentKey>,
    ) -> Result<Value, StoreError>;

    fn count(&self, set_name: &str, query: &ODataQuery, parent: Option<&ParentKey>) -> usize;

    // -- Single Entity CRUD --
    fn read_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        query: &ODataQuery,
    ) -> Result<Value, StoreError>;

    fn create_entity(
        &self,
        set_name: &str,
        data: &Value,
        parent: Option<&ParentKey>,
    ) -> Result<Value, StoreError>;

    fn patch_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        patch: &Value,
    ) -> Result<Value, StoreError>;

    fn delete_entity(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError>;

    // -- Draft Actions --
    fn draft_action(
        &self,
        set_name: &str,
        key: &EntityKey,
        action: DraftAction,
    ) -> Result<Value, StoreError>;

    // -- Sibling Entity (draft <-> active) --
    fn read_sibling_entity(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError>;

    // -- Property --
    fn get_property(
        &self,
        set_name: &str,
        key: &EntityKey,
        property: &str,
    ) -> Result<Value, StoreError>;

    // -- Persistence --
    fn commit(&self);

    // -- Default-Methoden (kein Implementierungszwang) --
    fn update_entities(
        &self,
        set_name: &str,
        updates: &[(EntityKey, Value)],
    ) -> Result<(), StoreError> {
        for (key, patch) in updates {
            self.patch_entity(set_name, key, patch)?;
        }
        Ok(())
    }

    fn seed_records(&self, set_name: &str, records: Vec<Value>) {
        for record in records {
            let _ = self.create_entity(set_name, &record, None);
        }
    }
}

/// Optionale Debug-Erweiterung. Bewusst getrennt vom Kern-Trait, damit
/// eine spaetere Postgres-Implementierung nicht gezwungen ist, einen
/// ungefilterten Full-Table-Scan anzubieten.
#[cfg(any(test, feature = "debug-tools"))]
pub trait DataStoreDebugExt: DataStore {
    fn get_records(&self, set_name: &str) -> Vec<Value>;
}
