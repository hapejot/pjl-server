//! Smalltalk-Klassen als Alternative zu Rhai-Skript-Hooks.

use std::sync::Arc;

use data_store::{DataRecord, EntityKey, StoreError};
use odata::query::ODataQuery;
use serde_json::{Value, json};
use smalltalk_hooks::{SmalltalkError, SmalltalkHooks};

use crate::extension::{ActionError, ActionHandler, BoundAction, Extension, HeaderButton};
use crate::hook::{CurrentRecords, Extender, Hook, HookError, Operation};

fn op_name(op: Operation) -> &'static str {
    match op {
        Operation::Create => "create",
        Operation::Patch => "patch",
        Operation::Delete => "delete",
        Operation::Read => "read",
        Operation::DraftEdit => "draft_edit",
        Operation::DraftActivate => "draft_activate",
        Operation::DraftPrepare => "draft_prepare",
    }
}

fn key_value(key: &EntityKey) -> Value {
    json!({"ID": key.id.to_string(), "IsActiveEntity": key.is_active_entity})
}

fn script_err(error: SmalltalkError) -> HookError {
    HookError::Hook(smalltalk_user_message(&error.to_string()))
}

fn smalltalk_user_message(message: &str) -> String {
    if let Some((_, detail)) = message.split_once("Smalltalk error: ") {
        return detail.split('\n').next().unwrap_or(detail).trim().to_string();
    }
    if let Some((detail, _)) = message.split_once("\nSmalltalk stack") {
        return detail
            .rsplit_once(": ")
            .map(|(_, detail)| detail.trim())
            .unwrap_or(detail.trim())
            .to_string();
    }
    message
        .rsplit_once(": ")
        .map(|(_, detail)| detail.trim())
        .unwrap_or(message.trim())
        .to_string()
}

fn record_to_value(record: &DataRecord) -> Result<Value, HookError> {
    serde_json::to_value(record)
        .map_err(|error| HookError::Store(StoreError::Serialize(error.to_string())))
}

fn record_from_value(value: &Value) -> Result<DataRecord, HookError> {
    Ok(DataRecord::from_json(value)?)
}

pub struct SmalltalkHook(pub Arc<SmalltalkHooks>);

#[async_trait::async_trait]
impl Hook for SmalltalkHook {
    async fn before(
        &self,
        _store: &Extender,
        set_name: &str,
        op: Operation,
        value: &mut Value,
    ) -> Result<(), HookError> {
        self.0.before(set_name, op_name(op), value).map_err(script_err)
    }

    async fn before_with_current(
        &self,
        _store: &Extender,
        set_name: &str,
        op: Operation,
        key: Option<&EntityKey>,
        value: &mut Value,
        current: &CurrentRecords,
    ) -> Result<(), HookError> {
        let current = json!({
            "active": current.active.as_ref().map(record_to_value).transpose()?,
            "draft": current.draft.as_ref().map(record_to_value).transpose()?,
        });
        self.0
            .before_operation(
                set_name,
                op_name(op),
                value,
                key.map(key_value),
                Some(&current),
            )
            .map_err(script_err)
    }

    async fn after(
        &self,
        _store: &Extender,
        set_name: &str,
        op: Operation,
        value: &mut DataRecord,
    ) -> Result<(), HookError> {
        let mut payload = record_to_value(value)?;
        if op == Operation::Read {
            self.0
                .after_read(set_name, "entity", None, None, &mut payload)
                .map_err(script_err)?;
        } else {
            self.0
                .after(set_name, op_name(op), &mut payload)
                .map_err(script_err)?;
        }
        *value = record_from_value(&payload)?;
        Ok(())
    }

    async fn before_read(
        &self,
        _store: &Extender,
        set_name: &str,
        key: Option<&EntityKey>,
        query: &mut ODataQuery,
    ) -> Result<(), HookError> {
        let mut value = serde_json::to_value(&*query)
            .map_err(|error| HookError::Store(StoreError::Serialize(error.to_string())))?;
        self.0
            .before_read(set_name, key.map(key_value), &mut value)
            .map_err(script_err)?;
        *query = serde_json::from_value(value)
            .map_err(|error| HookError::Hook(format!("{set_name}.before_read: query: {error}")))?;
        Ok(())
    }

    async fn after_read_list(
        &self,
        _store: &Extender,
        set_name: &str,
        records: &mut Vec<DataRecord>,
    ) -> Result<(), HookError> {
        let mut payload = Value::Array(
            records
                .iter()
                .map(record_to_value)
                .collect::<Result<Vec<_>, _>>()?,
        );
        self.0
            .after_read(set_name, "list", None, None, &mut payload)
            .map_err(script_err)?;
        let Value::Array(items) = payload else {
            return Err(HookError::Hook("after_read: Liste erwartet".into()));
        };
        *records = items
            .iter()
            .map(record_from_value)
            .collect::<Result<_, _>>()?;
        Ok(())
    }

    async fn after_property(
        &self,
        _store: &Extender,
        set_name: &str,
        key: &EntityKey,
        property: &str,
        value: &mut Value,
    ) -> Result<(), HookError> {
        self.0
            .after_read(set_name, "property", Some(key_value(key)), Some(property), value)
            .map_err(script_err)
    }
}

pub struct SmalltalkExtension {
    hooks: Arc<SmalltalkHooks>,
    actions: Vec<BoundAction>,
}

impl SmalltalkExtension {
    pub fn new(hooks: SmalltalkHooks) -> Self {
        Self {
            hooks: Arc::new(hooks),
            actions: Vec::new(),
        }
    }

    pub fn with_actions(
        mut self,
        ddic: &spec::data_dictionary::DataDictionary,
    ) -> anyhow::Result<Self> {
        for entity in ddic.entities() {
            let mut names = std::collections::HashSet::new();
            for action in entity.actions() {
                let mut chars = action.name.chars();
                anyhow::ensure!(
                    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_'),
                    "{}: ungueltiger Aktionsname '{}'",
                    entity.name(),
                    action.name
                );
                anyhow::ensure!(
                    names.insert(&action.name),
                    "{}: doppelte Aktion '{}'",
                    entity.name(),
                    action.name
                );
                anyhow::ensure!(
                    !action.label.trim().is_empty(),
                    "{}.{}: Beschriftung fehlt",
                    entity.name(),
                    action.name
                );
                let set = entity.setname();
                anyhow::ensure!(
                    self.hooks.has_action(&set, &action.name),
                    "{set}: Methode action_{} fehlt",
                    action.name
                );
                self.actions.push(BoundAction {
                    set: set.clone(),
                    name: action.name.clone(),
                    returns: Some(entity.name().to_string()),
                    handler: Arc::new(SmalltalkAction {
                        hooks: self.hooks.clone(),
                        set,
                        name: action.name.clone(),
                    }),
                    header: action.header.then(|| HeaderButton {
                        text: action.label.clone(),
                    }),
                    available_if: None,
                });
            }
        }
        Ok(self)
    }
}

struct SmalltalkAction {
    hooks: Arc<SmalltalkHooks>,
    set: String,
    name: String,
}

#[async_trait::async_trait]
impl ActionHandler for SmalltalkAction {
    async fn invoke(&self, state: &crate::AppState, key: &EntityKey) -> Result<Value, ActionError> {
        if !key.is_active_entity {
            return Err(ActionError::Failed(
                "Aktionen sind nur fuer aktive Datensaetze erlaubt".into(),
            ));
        }
        let ddic = state.ddic();
        let entity = ddic
            .active_entity_by_setname(&self.set)
            .ok_or_else(|| ActionError::Failed(format!("Unbekanntes Set '{}'", self.set)))?;
        if entity.draft_enabled() {
            match state.store.read_sibling_entity(&self.set, key).await {
                Ok(_) => {
                    return Err(ActionError::Failed(
                        "Der Datensatz hat einen offenen Entwurf".into(),
                    ));
                }
                Err(StoreError::NotFound { .. }) => {}
                Err(error) => return Err(error.into()),
            }
        }
        let record = state.store.read_entity(&self.set, key, &ODataQuery::new()).await?;
        let record = serde_json::to_value(record).map_err(|error| ActionError::Failed(error.to_string()))?;
        let patch = self
            .hooks
            .action(&self.set, &self.name, record)
            .map_err(|error| ActionError::Failed(error.to_string()))?;
        let fields = patch
            .as_object()
            .ok_or_else(|| ActionError::Failed("Patch muss ein Objekt sein".into()))?;
        for field in fields.keys() {
            let writable = match entity.field_by_name(field) {
                Some(spec::FieldSpec::Atom { computed, immutable, .. }) => !computed && !immutable,
                Some(spec::FieldSpec::Measure { .. }) => true,
                _ => false,
            };
            if !writable || entity.primary_key_name() == field {
                return Err(ActionError::Failed(format!(
                    "Feld '{field}' kann durch eine Aktion nicht geaendert werden"
                )));
            }
        }
        state.store.patch_entity(&self.set, key, &patch).await?;
        let updated = state.store.read_entity(&self.set, key, &ODataQuery::new()).await?;
        serde_json::to_value(updated).map_err(|error| ActionError::Failed(error.to_string()))
    }
}

impl Extension for SmalltalkExtension {
    fn actions(&self) -> Vec<BoundAction> {
        self.actions.clone()
    }

    fn hooks(&self) -> Vec<Arc<dyn Hook>> {
        vec![Arc::new(SmalltalkHook(self.hooks.clone()))]
    }

    fn status(&self) -> Option<(String, Value)> {
        Some(("smalltalk".to_string(), self.hooks.status()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smalltalk_error_text_is_user_friendly() {
        let raw = "AccountSet.beforeDraftPrepare:: Runtime: Smalltalk error: Stadt ist nicht Hannover\nSmalltalk stack (innermost first):\n  at False>>ifFalse:";
        assert_eq!(
            smalltalk_user_message(raw),
            "Stadt ist nicht Hannover"
        );
    }

    #[test]
    fn hook_error_does_not_prefix_audit() {
        let error: StoreError =
            HookError::Hook("Stadt ist nicht Hannover".to_string()).into();
        assert_eq!(error.to_string(), "Conflict: Stadt ist nicht Hannover");
    }
}
