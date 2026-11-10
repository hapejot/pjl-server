use std::sync::Arc;

use serde_json::{Value, json};
use thiserror::Error;
use tracing::debug;

use data_store::{
    DataRecord, DataStore, DraftDataStore, EntityKey, Navigation, StoreError, WriteOp,
};
use odata::query::ODataQuery;

/// Bestehende Datensaetze mit derselben ID fuer einen Before-Hook.
#[derive(Debug, Clone, Default)]
pub struct CurrentRecords {
    pub active: Option<DataRecord>,
    pub draft: Option<DataRecord>,
}

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
    #[error("Hook Fehler: {0}")]
    Hook(String),
    /// Originale Store-Fehler; wird so weitergeleitet (Cycle vermieden): kein `StoreError`.
    #[error("Store Fehler: {0}")]
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

#[async_trait::async_trait]
pub trait Hook: Send + Sync {
    async fn before(
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

    /// Schreiboperation vor dem Store, einschliesslich beider vorhandener
    /// Varianten des Datensatzes. Bestehende Hooks behalten ihr bisheriges
    /// Verhalten, sofern sie diese Methode nicht ueberschreiben.
    async fn before_with_current(
        &self,
        store: &Extender,
        set_name: &str,
        op: Operation,
        key: Option<&EntityKey>,
        value: &mut Value,
        current: &CurrentRecords,
    ) -> Result<(), HookError> {
        if op == Operation::Patch {
            if let Some(key) = key {
                return self.before_patch(store, set_name, key, value).await;
            }
        }
        let _ = current;
        self.before(store, set_name, op, value).await
    }

    async fn after(
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

    /// Patch mit bekanntem Schluessel. Standard: wie [`Hook::before`] mit `Operation::Patch`.
    async fn before_patch(
        &self,
        store: &Extender,
        set_name: &str,
        key: &EntityKey,
        patch: &mut Value,
    ) -> Result<(), HookError> {
        let _ = key;
        self.before(store, set_name, Operation::Patch, patch).await
    }

    /// Lesezugriff, einmal pro Request, vor dem Store. `key` ist gesetzt bei
    /// Einzelzugriff, `None` bei Listen. Die Query darf ergaenzt oder reduziert werden.
    async fn before_read(
        &self,
        store: &Extender,
        set_name: &str,
        key: Option<&EntityKey>,
        query: &mut ODataQuery,
    ) -> Result<(), HookError> {
        let _ = (store, set_name, key, query);
        Ok(())
    }

    /// Ergebnisliste eines Lesezugriffs, einmal pro Request. Saetze duerfen
    /// geaendert und entfernt werden.
    async fn after_read_list(
        &self,
        store: &Extender,
        set_name: &str,
        records: &mut Vec<DataRecord>,
    ) -> Result<(), HookError> {
        let _ = (store, set_name, records);
        Ok(())
    }

    /// Einzelne Property-Lesung (`$value`), einmal pro Request.
    async fn after_property(
        &self,
        store: &Extender,
        set_name: &str,
        key: &EntityKey,
        property: &str,
        value: &mut Value,
    ) -> Result<(), HookError> {
        let _ = (store, set_name, key, property, value);
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
    /// Aktueller Datensatz als JSON (fuer Hooks, die vor einem Patch den Bestand brauchen).
    pub async fn current_value(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<Value, StoreError> {
        let record = self
            .inner
            .read_entity(set_name, key, &odata::query::ODataQuery::new())
            .await?;
        serde_json::to_value(&record).map_err(|e| StoreError::Serialize(e.to_string()))
    }

    /// Beide vorhandenen Varianten eines Datensatzes fuer einen Before-Hook.
    pub async fn current_records(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<CurrentRecords, StoreError> {
        let active = self
            .optional_record(set_name, &EntityKey::new(key.id.clone(), true))
            .await?;
        let draft = self
            .optional_record(set_name, &EntityKey::new(key.id.clone(), false))
            .await?;
        Ok(CurrentRecords { active, draft })
    }

    async fn optional_record(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<Option<DataRecord>, StoreError> {
        match self
            .inner
            .read_entity(set_name, key, &ODataQuery::default())
            .await
        {
            Ok(record) => Ok(Some(record)),
            Err(StoreError::NotFound { .. } | StoreError::InvalidState) => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn current_for_value(
        &self,
        set_name: &str,
        value: &Value,
    ) -> Result<CurrentRecords, StoreError> {
        match EntityKey::new_from_json(value) {
            Some(key) => self.current_records(set_name, &key).await,
            None => Ok(CurrentRecords::default()),
        }
    }

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

    /// Darunterliegender Store (ohne diesen Hook). Fuer Hooks, die selbst lesen
    /// oder schreiben muessen, ohne sich rekursiv aufzurufen.
    pub(crate) fn raw(&self) -> &Arc<dyn DraftDataStore> {
        &self.inner
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

#[async_trait::async_trait]
impl DraftDataStore for Extender {
    async fn draft_edit(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError> {
        let mut value = Self::key_value(key);
        let current = self.current_records(set_name, key).await?;
        self.hook
            .before_with_current(
                self,
                set_name,
                Operation::DraftEdit,
                Some(key),
                &mut value,
                &current,
            )
            .await?;
        let result = self.inner.draft_edit(set_name, key).await?;
        let mut record = DataRecord::from_json(&result)?;
        self.hook
            .after(self, set_name, Operation::DraftEdit, &mut record)
            .await?;
        Ok(result)
    }

    async fn draft_activate(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError> {
        let mut value = Self::key_value(key);
        let current = self.current_records(set_name, key).await?;
        self.hook
            .before_with_current(
                self,
                set_name,
                Operation::DraftActivate,
                Some(key),
                &mut value,
                &current,
            )
            .await?;
        let result = self.inner.draft_activate(set_name, key).await?;
        let mut record = DataRecord::from_json(&result)?;
        self.hook
            .after(self, set_name, Operation::DraftActivate, &mut record)
            .await?;
        Ok(result)
    }

    async fn draft_prepare(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError> {
        let mut value = json!({ "key": Self::key_value(key) });
        let current = self.current_records(set_name, key).await?;

        self.hook
            .before_with_current(
                self,
                set_name,
                Operation::DraftPrepare,
                Some(key),
                &mut value,
                &current,
            )
            .await?;
        let result = self.inner.draft_prepare(set_name, key).await?;
        let mut record = DataRecord::from_json(&result)?;
        self.hook
            .after(self, set_name, Operation::DraftPrepare, &mut record)
            .await?;
        Ok(result)
    }
}

#[async_trait::async_trait]
impl DataStore for Extender {
    fn component_info(&self) -> String {
        format!("Extender -> {}", self.inner.component_info())
    }

    async fn get_collection(
        &self,
        set_name: &str,
        query: &odata::query::ODataQuery,
    ) -> Result<Vec<DataRecord>, StoreError> {
        Ok(self.read_collection(set_name, query, false).await?.0)
    }

    /// Ein Request = ein `before_read` (Query), Store, ein `after_read_list`.
    /// Die Gesamtzahl wird um die durch den Hook entfernten Saetze verringert.
    async fn read_collection(
        &self,
        set_name: &str,
        query: &odata::query::ODataQuery,
        with_count: bool,
    ) -> Result<(Vec<DataRecord>, Option<usize>), StoreError> {
        let mut q = query.clone();
        self.hook.before_read(self, set_name, None, &mut q).await?;
        let mut records = self.inner.get_collection(set_name, &q).await?;
        let before = records.len();
        self.hook
            .after_read_list(self, set_name, &mut records)
            .await?;
        let removed = before.saturating_sub(records.len());
        let count = if with_count {
            Some(self.inner.count(set_name, &q).await.saturating_sub(removed))
        } else {
            None
        };
        Ok((records, count))
    }

    async fn count(&self, set_name: &str, query: &odata::query::ODataQuery) -> usize {
        self.inner.count(set_name, query).await
    }

    async fn read_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        query: &odata::query::ODataQuery,
    ) -> Result<DataRecord, StoreError> {
        let mut q = query.clone();
        self.hook
            .before_read(self, set_name, Some(key), &mut q)
            .await?;
        let mut record = self.inner.read_entity(set_name, key, &q).await?;
        self.hook
            .after(self, set_name, Operation::Read, &mut record)
            .await?;
        Ok(record)
    }

    async fn create_entity(&self, set_name: &str, data: &Value) -> Result<DataRecord, StoreError> {
        let mut value = data.clone();
        let key = EntityKey::new_from_json(&value);
        let current = self.current_for_value(set_name, &value).await?;
        self.hook
            .before_with_current(
                self,
                set_name,
                Operation::Create,
                key.as_ref(),
                &mut value,
                &current,
            )
            .await?;
        let mut r = self.inner.create_entity(set_name, &value).await?;
        self.hook
            .after(self, set_name, Operation::Create, &mut r)
            .await?;
        Ok(r)
    }

    async fn patch_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        patch: &Value,
    ) -> Result<Value, StoreError> {
        let mut value = patch.clone();
        let current = self.current_records(set_name, key).await?;
        self.hook
            .before_with_current(
                self,
                set_name,
                Operation::Patch,
                Some(key),
                &mut value,
                &current,
            )
            .await?;
        let result = self.inner.patch_entity(set_name, key, &value).await?;
        let mut record = self
            .inner
            .read_entity(set_name, key, &odata::query::ODataQuery::new())
            .await?;
        self.hook
            .after(self, set_name, Operation::Patch, &mut record)
            .await?;
        Ok(result)
    }

    async fn delete_entity(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        let mut value = Self::key_value(key);
        let current = self.current_records(set_name, key).await?;
        self.hook
            .before_with_current(
                self,
                set_name,
                Operation::Delete,
                Some(key),
                &mut value,
                &current,
            )
            .await?;
        let result = self.inner.delete_entity(set_name, key).await;
        result?;
        let mut record = Self::key_record(key);
        self.hook
            .after(self, set_name, Operation::Delete, &mut record)
            .await?;
        Ok(())
    }

    /// Batch-Variante: Before-Hooks laufen pro Satz, die verbliebenen Operationen
    /// gehen als Liste an den inneren Store, danach laufen After-Hooks pro Satz.
    /// Ein Hook-Fehler betrifft nur seinen Satz.
    async fn write_entities(
        &self,
        set_name: &str,
        ops: Vec<WriteOp>,
    ) -> Vec<Result<Option<DataRecord>, StoreError>> {
        let mut results: Vec<Option<Result<Option<DataRecord>, StoreError>>> =
            (0..ops.len()).map(|_| None).collect();
        // Pro weitergereichter Operation: Index in `ops`, Operationsart, Schluessel bei Delete.
        let mut pending: Vec<(usize, Operation, Option<EntityKey>)> = Vec::new();
        let mut inner_ops = Vec::new();

        for (i, op) in ops.into_iter().enumerate() {
            let (kind, mut value, key) = match op {
                WriteOp::Create(data) => (Operation::Create, data, None),
                WriteOp::Patch(key, patch) => (Operation::Patch, patch, Some(key)),
                WriteOp::Delete(key) => (Operation::Delete, Self::key_value(&key), Some(key)),
            };
            let current = match key.as_ref() {
                Some(key) => self.current_records(set_name, key).await,
                None => self.current_for_value(set_name, &value).await,
            };
            let before = match current {
                Ok(current) => {
                    self.hook
                        .before_with_current(
                            self,
                            set_name,
                            kind,
                            key.as_ref(),
                            &mut value,
                            &current,
                        )
                        .await
                }
                Err(error) => Err(error.into()),
            };
            if let Err(e) = before {
                results[i] = Some(Err(e.into()));
                continue;
            }
            // Vom Before-Hook geaenderte Werte gehen an den inneren Store.
            inner_ops.push(match (kind, &key) {
                (Operation::Create, _) => WriteOp::Create(value),
                (Operation::Patch, Some(k)) => WriteOp::Patch(k.clone(), value),
                (Operation::Delete, Some(k)) => WriteOp::Delete(k.clone()),
                _ => unreachable!("Patch und Delete haben immer einen Schluessel"),
            });
            pending.push((i, kind, key));
        }

        let inner_results = self.inner.write_entities(set_name, inner_ops).await;
        for ((i, kind, key), result) in pending.into_iter().zip(inner_results) {
            let result = match result {
                Ok(Some(mut record)) => {
                    match self.hook.after(self, set_name, kind, &mut record).await {
                        Ok(()) => Ok(Some(record)),
                        Err(e) => Err(e.into()),
                    }
                }
                Ok(None) => {
                    // Delete: After-Hook mit dem Schluessel-Datensatz, wie in delete_entity.
                    let mut record =
                        Self::key_record(key.as_ref().expect("Delete hat einen Schluessel"));
                    match self.hook.after(self, set_name, kind, &mut record).await {
                        Ok(()) => Ok(None),
                        Err(e) => Err(e.into()),
                    }
                }
                Err(e) => Err(e),
            };
            results[i] = Some(result);
        }
        results
            .into_iter()
            .map(|r| r.expect("jede Operation hat ein Ergebnis"))
            .collect()
    }

    async fn delete_entity_tree_only(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<(), StoreError> {
        let mut value = Self::key_value(key);
        let current = self.current_records(set_name, key).await?;
        self.hook
            .before_with_current(
                self,
                set_name,
                Operation::Delete,
                Some(key),
                &mut value,
                &current,
            )
            .await?;
        let result = self.inner.delete_entity_tree_only(set_name, key).await;
        result?;
        let mut record = Self::key_record(key);
        self.hook
            .after(self, set_name, Operation::Delete, &mut record)
            .await?;
        Ok(())
    }

    async fn read_sibling_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<DataRecord, StoreError> {
        let mut q = ODataQuery::new();
        self.hook
            .before_read(self, set_name, Some(key), &mut q)
            .await?;
        let mut record = self.inner.read_sibling_entity(set_name, key).await?;
        self.hook
            .after(self, set_name, Operation::Read, &mut record)
            .await?;
        Ok(record)
    }

    async fn get_property(
        &self,
        set_name: &str,
        key: &EntityKey,
        property: &str,
    ) -> Result<Value, StoreError> {
        let mut value = self.inner.get_property(set_name, key, property).await?;
        self.hook
            .after_property(self, set_name, key, property, &mut value)
            .await?;
        Ok(value)
    }

    async fn commit(&self) {
        self.inner.commit().await;
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

#[async_trait::async_trait]
impl Hook for DefaultHook {
    async fn before(
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

    async fn after(
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

#[cfg(test)]
mod test;
