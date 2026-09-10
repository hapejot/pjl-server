use std::sync::Arc;

use serde_json::Value;
use thiserror::Error;
use tracing::debug;

use odata_yaml_store::datastore::{DataRecord, Navigation};
use odata_yaml_store::key::EntityKey;
use odata_yaml_store::{DataStore, DraftDataStore, StoreError};

/// Identifiziert eine durchlaufende Operation (für Logging/Vorschau im Hook).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Create,
    Patch,
    /// Lesezugriffe prüfen/erweitern (kein Vorab-Zustand).
    Read,
    Delete,
    DraftEdit,
    DraftActivate,
}

/// Ergebnis einer Audit-Zwischenoperation.
#[derive(Debug, Error)]
pub enum HookError {
    /// Deine eigene Prüferenz (z. B. FK-Verstoß / berechneter Wert).
    #[error("Hook Fehler: &1")]
    Hook(String),
    /// Originale Store-Fehler; wird so weitergeleitet (Cycle vermieden): kein `StoreError`.
    #[error("Store Fehler: &1")]
    Store(StoreError),
}

impl From<StoreError> for HookError {
    fn from(e: StoreError) -> Self {
        HookError::Store(e)
    }
}

impl From<HookError> for StoreError {
    fn from(e: HookError) -> Self {
        match e {
            HookError::Store(s) => s,
            HookError::Hook(msg) => StoreError::Conflict(format!("Audit: {msg}")),
        }
    }
}

pub trait Hook: Send + Sync {
    fn before(
        &self,
        store: &Extender,
        set_name: &str,
        op: Operation,
        value: &mut Value,
    ) -> Result<(), HookError> {
        let _ = store;
        debug!(set_name, ?op, ?value);
        Ok(())
    }
    fn after(
        &self,
        store: &Extender,
        set_name: &str,
        op: Operation,
        value: &mut DataRecord,
    ) -> Result<(), HookError> {
        let _ = store;
        debug!(set_name, ?op, ?value);
        Ok(())
    }
}

/// Delegationstyp: kapselt einen `Arc<S>` und ruft an CRUD-relevanten Stellen `EntityAudit::audit` aus.
///
/// Implementiert die drei relevanten Traits, leitet alles an `self.inner` weiter und
/// packt `before`/`after` an den Hook. Der Hook kann `Ok(e)` zurückgeben, wenn er
/// seinen eigenen Fehler wirft (z. B. Prüfung misslungen); Store-Fehler werden zuerst durchgereicht.
#[derive(Clone)]
pub struct Extender {
    inner: Arc<dyn DraftDataStore>,
    hook: Arc<dyn Hook>,
}

impl Extender {
    pub fn new(
        inner: Arc<dyn DraftDataStore>,
        hook: Arc<dyn Hook>,
    ) -> Result<Arc<dyn DraftDataStore>, HookError> {
        Ok(Arc::new(Extender { inner, hook }))
    }
}

impl DraftDataStore for Extender {
    fn draft_edit(
        &self,
        _set_name: &str,
        _key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError> {
        todo!()
    }

    fn draft_activate(
        &self,
        _set_name: &str,
        _key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError> {
        todo!()
    }

    fn draft_prepare(
        &self,
        _set_name: &str,
        _key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError> {
        todo!()
    }
}

impl DataStore for Extender {
    fn get_collection(
        &self,
        set_name: &str,
        query: &odata::query::ODataQuery,
    ) -> Result<Vec<odata_yaml_store::datastore::DataRecord>, StoreError> {
        self.inner.get_collection(set_name, query)
    }

    fn count(&self, set_name: &str, query: &odata::query::ODataQuery) -> usize {
        self.inner.count(set_name, query)
    }

    fn read_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        query: &odata::query::ODataQuery,
    ) -> Result<odata_yaml_store::datastore::DataRecord, StoreError> {
        self.inner.read_entity(set_name, key, query)
    }

    fn create_entity(
        &self,
        set_name: &str,
        data: &Value,
    ) -> Result<odata_yaml_store::datastore::DataRecord, StoreError> {
        let mut value = data.clone();
        self.hook
            .before(self, set_name, Operation::Create, &mut value)?;
        let mut r = self.inner.create_entity(set_name, data)?;
        self.hook.after(self, set_name, Operation::Create, &mut r)?;
        Ok(r)
    }

    fn patch_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        patch: &Value,
    ) -> Result<Value, StoreError> {
        self.inner.patch_entity(set_name, key, patch)
    }

    fn delete_entity(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        self.inner.delete_entity(set_name, key)
    }

    fn read_sibling_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<odata_yaml_store::datastore::DataRecord, StoreError> {
        self.inner.read_sibling_entity(set_name, key)
    }

    fn get_property(
        &self,
        set_name: &str,
        key: &EntityKey,
        property: &str,
    ) -> Result<Value, StoreError> {
        self.inner.get_property(set_name, key, property)
    }

    fn commit(&self) {
        todo!()
    }

    fn navigation(&mut self, _single: bool) -> &mut Navigation {
        todo!()
    }

    fn set_navigations(&self, _navigations: Vec<Navigation>) {
        todo!()
    }
}
#[cfg(test)]
mod test {
    use std::sync::RwLock;

    use odata_yaml_store::YamlStore;
    use serde_json::json;
    use tracing::Level;

    use super::*;
    /// Ein Beispiel-Sink, der das Skelett füllbar lässt: jeder Hook speichert Zahlen, die du testweise
    /// abrufen und für Prüfungen nutzen kannst. Dein eigener Hook kopiert hier diese Felder.
    #[derive(Default)]
    struct HookData {
        pub created: u64,
        pub activated: u64,
    }
    #[derive(Default)]
    pub struct ExampleHook {
        inner: RwLock<HookData>,
    }

    impl ExampleHook {
        pub fn create_count(&self) -> u64 {
            self.inner.try_read().unwrap().created
        }
    }

    impl Hook for ExampleHook {
        fn after(
            &self,
            store: &Extender,
            set_name: &str,
            op: Operation,
            value: &mut DataRecord,
        ) -> Result<(), HookError> {
            debug!(?op, ?value);
            
            // Track whether we need to create an OrderItem
            let should_create_item = {
                let mut w = self.inner.try_write().map_err(|_| {
                    HookError::Hook("Failed to acquire write lock".to_string())
                })?;
                
                match set_name {
                    "OrderSet" => match op {
                        Operation::Create => {
                            w.created += 1;
                            true
                        }
                        Operation::DraftActivate => {
                            w.activated += 1;
                            false
                        }
                        _ => false
                    },
                    _ => false
                }
            }; // Lock is released here
            
            // Create OrderItem outside the lock
            if should_create_item {
                store.create_entity(
                    "OrderItemSet",
                    &json!({"Pos": "10", "Order_ID": value.id()}),
                )?;
            }
            
            Ok(())
        }
    }

    #[test]
    fn example_hook() -> anyhow::Result<()> {
        tracing_subscriber::fmt()
            .with_max_level(Level::TRACE)
            .init();
        let ystore = Arc::new(YamlStore::open("testdata")?);
        let hook = Arc::new(ExampleHook::default());
        let store = Extender::new(ystore, hook.clone())?;
        store.create_entity("OrderSet", &json!({"SoldTo": "Peter"}))?;
        assert_eq!(hook.create_count(), 1);
        Ok(())
    }
}
