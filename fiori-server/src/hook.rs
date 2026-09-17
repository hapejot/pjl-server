use std::sync::Arc;

use serde_json::{json, Value};
use thiserror::Error;
use tracing::debug;

use data_store::{DataRecord, DataStore, DraftDataStore, EntityKey, Navigation, StoreError};

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
    DraftPrepare,
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
    navigation: Navigation,
}

impl Extender {
    /// Erzeugt einen neuen [`Extender`], der einen bestehenden Store mit dem
    /// angegebenen [`Hook`] verdrahtet.
    pub fn new(
        inner: Arc<dyn DraftDataStore>,
        hook: Arc<dyn Hook>,
    ) -> Result<Arc<dyn DraftDataStore>, HookError> {
        Ok(Arc::new(Extender::build(inner, hook)))
    }

    /// Konstruiert einen [`Extender`] ohne `Arc`-Huehulli (nuerzuelich fuer
    /// Tests / interne Nutzung).
    pub fn build(inner: Arc<dyn DraftDataStore>, hook: Arc<dyn Hook>) -> Self {
        Extender {
            inner,
            hook,
            navigation: Navigation::default(),
        }
    }

    /// JSON-Key-Payload für `before`-Hooks.
    fn key_value(key: &EntityKey) -> Value {
        json!({
            "ID": key.id.to_string(),
            "IsActiveEntity": key.is_active_entity,
        })
    }

    /// Synthisiert ein [`DataRecord`] aus einem [`EntityKey`].
    /// Wird als `after`-Payload für Operationen verwendet, die keinen
    /// Rückgabewert liefern (Delete).
    fn key_record(key: &EntityKey) -> DataRecord {
        DataRecord::from_json(&Self::key_value(key)).expect("key payload")
    }
}

impl DraftDataStore for Extender {
    fn draft_edit(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError> {
        let mut value = Self::key_value(key);
        self.hook
            .before(self, set_name, Operation::DraftEdit, &mut value)?;
        let result = self.inner.draft_edit(set_name, key)?;
        let mut record = DataRecord::from_json(&result)?;
        self.hook.after(self, set_name, Operation::DraftEdit, &mut record)?;
        Ok(result)
    }

    fn draft_activate(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError> {
        let mut value = Self::key_value(key);
        self.hook
            .before(self, set_name, Operation::DraftActivate, &mut value)?;
        let result = self.inner.draft_activate(set_name, key)?;
        let mut record = DataRecord::from_json(&result)?;
        self.hook.after(self, set_name, Operation::DraftActivate, &mut record)?;
        Ok(result)
    }

    fn draft_prepare(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError> {
        let mut value = Self::key_value(key);
        self.hook
            .before(self, set_name, Operation::DraftPrepare, &mut value)?;
        let result = self.inner.draft_prepare(set_name, key)?;
        let mut record = DataRecord::from_json(&result)?;
        self.hook.after(self, set_name, Operation::DraftPrepare, &mut record)?;
        Ok(result)
    }
}

impl DataStore for Extender {
    fn get_collection(
        &self,
        set_name: &str,
        query: &odata::query::ODataQuery,
    ) -> Result<Vec<DataRecord>, StoreError> {
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
    ) -> Result<DataRecord, StoreError> {
        self.inner.read_entity(set_name, key, query)
    }

    fn create_entity(
        &self,
        set_name: &str,
        data: &Value,
    ) -> Result<DataRecord, StoreError> {
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
        let mut value = patch.clone();
        self.hook
            .before(self, set_name, Operation::Patch, &mut value)?;
        let result = self.inner.patch_entity(set_name, key, patch)?;
        let mut record = self
            .inner
            .read_entity(set_name, key, &odata::query::ODataQuery::new())?;
        self.hook.after(self, set_name, Operation::Patch, &mut record)?;
        Ok(result)
    }

    fn delete_entity(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        let mut value = Self::key_value(key);
        self.hook
            .before(self, set_name, Operation::Delete, &mut value)?;
        let result = self.inner.delete_entity(set_name, key);
        result?;
        let mut record = Self::key_record(key);
        self.hook.after(self, set_name, Operation::Delete, &mut record)?;
        Ok(())
    }

    fn delete_entity_tree_only(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<(), StoreError> {
        let mut value = Self::key_value(key);
        self.hook
            .before(self, set_name, Operation::Delete, &mut value)?;
        let result = self.inner.delete_entity_tree_only(set_name, key);
        result?;
        let mut record = Self::key_record(key);
        self.hook.after(self, set_name, Operation::Delete, &mut record)?;
        Ok(())
    }

    fn read_sibling_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<DataRecord, StoreError> {
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
        self.inner.commit();
    }

    fn navigation(&mut self, single: bool) -> &mut Navigation {
        self.navigation.single = single;
        &mut self.navigation
    }

    fn set_navigations(&self, navigations: Vec<Navigation>) {
        self.inner.set_navigations(navigations);
    }
}

/// Produktions-Hook: verandert nichts, loggt aber jede durchlaufende
/// CRUD/Draft-Operation. So laesst sich das Hook-System aktivieren, ohne
/// das Verhalten des Stores zu aendern.
#[derive(Default)]
pub struct DefaultHook;

impl Hook for DefaultHook {
    fn before(
        &self,
        store: &Extender,
        set_name: &str,
        op: Operation,
        value: &mut Value,
    ) -> Result<(), HookError> {
        debug!(set_name, ?op, ?value, "before");
        let _ = store;
        Ok(())
    }

    fn after(
        &self,
        store: &Extender,
        set_name: &str,
        op: Operation,
        value: &mut DataRecord,
    ) -> Result<(), HookError> {
        debug!(set_name, ?op, ?value, "after");
        let _ = store;
        Ok(())
    }
}

/// Beim Anlegen eines `EntitySet`-Datensatzes werden automatisch zwei
/// `FieldSet`-Kinder angelegt (`name` und `title`). Die Kinder erben den
/// Draft-Zustand des Parents; Nutzer koennen sie spaeter umbenennen oder
/// loeschen.
///
/// Semantik:
/// * feuert im `after`-Callback von [`Operation::Create`] fuer `set_name ==
///   "EntitySet"`,
/// * legt die Kinder ueber [`Extender::create_entity`] an -- damit gelten
///   dieselben Hook-/Validierungspfade wie bei jedem anderen Kind-Create,
/// * uebernimmt den `IsActiveEntity`-Zustand des Parents; im Draft-Modus
///   entstehen also Draft-Kinder, aktiv angelegte Parents erhalten aktive
///   Kinder.
#[derive(Default)]
pub struct EntityDefaultsHook;

impl EntityDefaultsHook {
    const PARENT_SET: &'static str = "EntitySet";
    const CHILD_SET: &'static str = "FieldSet";
    /// Foreign-Key-Spalte in `FieldSet`, die auf das Parent-`EntitySet` zeigt.
    const CHILD_FK: &'static str = "entity_ID";
    /// Namen der automatisch angelegten Felder.
    const DEFAULT_FIELDS: &'static [&'static str] = &["name", "title"];
}

impl Hook for EntityDefaultsHook {
    fn after(
        &self,
        store: &Extender,
        set_name: &str,
        op: Operation,
        value: &mut DataRecord,
    ) -> Result<(), HookError> {
        // Nur beim Anlegen eines EntitySet greifen. Draft-Aktivierung und
        // Draft-Prepare beruehren den Datensatz nicht neu: die Kinder wurden
        // beim urspruenglichen Create schon mitgeschrieben und werden vom
        // Draft-Aktivierungspfad zusammen mit dem Parent uebernommen.
        if set_name != Self::PARENT_SET || op != Operation::Create {
            return Ok(());
        }
        let parent_id = value.id().to_string();
        let is_active = value.is_active_entity();
        for field_name in Self::DEFAULT_FIELDS {
            store.create_entity(
                Self::CHILD_SET,
                &json!({
                    "name": field_name,
                    Self::CHILD_FK: parent_id,
                    "IsActiveEntity": is_active,
                }),
            )?;
        }
        Ok(())
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
