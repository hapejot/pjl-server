//! Anbindung der Rhai-Skripte (`script-hooks`) an den Store-Hook und an die
//! Erweiterungen des Servers. Der Adapter uebersetzt Store-Operationen in
//! Werte fuer die Skripte und zurueck.

use std::sync::Arc;

use data_store::{DataRecord, EntityKey, StoreError};
use odata::query::ODataQuery;
use script_hooks::ScriptHooks;
use serde_json::{Value, json};

use crate::extension::Extension;
use crate::hook::{Extender, Hook, HookError, Operation};
use crate::term3270::section_view;

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
    json!({ "ID": key.id.to_string(), "IsActiveEntity": key.is_active_entity })
}

fn script_err(e: script_hooks::ScriptError) -> HookError {
    HookError::Hook(e.to_string())
}

fn record_to_value(r: &DataRecord) -> Result<Value, HookError> {
    serde_json::to_value(r).map_err(|e| HookError::Store(StoreError::Serialize(e.to_string())))
}

fn record_from_value(v: &Value) -> Result<DataRecord, HookError> {
    Ok(DataRecord::from_json(v)?)
}

/// Store-Hook, der die Skripte aufruft.
pub struct ScriptHook(pub Arc<ScriptHooks>);

#[async_trait::async_trait]
impl Hook for ScriptHook {
    async fn before(
        &self,
        _store: &Extender,
        set_name: &str,
        op: Operation,
        value: &mut Value,
    ) -> Result<(), HookError> {
        self.0.before(set_name, op_name(op), value).map_err(script_err)
    }

    async fn after(
        &self,
        _store: &Extender,
        set_name: &str,
        op: Operation,
        value: &mut DataRecord,
    ) -> Result<(), HookError> {
        let mut v = record_to_value(value)?;
        if op == Operation::Read {
            self.0
                .after_read(set_name, "entity", None, None, &mut v)
                .map_err(script_err)?;
        } else {
            self.0.after(set_name, op_name(op), &mut v).map_err(script_err)?;
        }
        *value = record_from_value(&v)?;
        Ok(())
    }

    async fn before_read(
        &self,
        _store: &Extender,
        set_name: &str,
        key: Option<&EntityKey>,
        query: &mut ODataQuery,
    ) -> Result<(), HookError> {
        self.0
            .before_read(set_name, key.map(key_value), query)
            .map_err(script_err)
    }

    async fn after_read_list(
        &self,
        _store: &Extender,
        set_name: &str,
        records: &mut Vec<DataRecord>,
    ) -> Result<(), HookError> {
        let mut v = Value::Array(
            records
                .iter()
                .map(record_to_value)
                .collect::<Result<Vec<_>, _>>()?,
        );
        self.0
            .after_read(set_name, "list", None, None, &mut v)
            .map_err(script_err)?;
        let Value::Array(items) = v else {
            return Err(HookError::Hook("after_read: Liste erwartet".into()));
        };
        *records = items.iter().map(record_from_value).collect::<Result<_, _>>()?;
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
            .after_read(
                set_name,
                "property",
                Some(key_value(key)),
                Some(property),
                value,
            )
            .map_err(script_err)
    }
}

/// Erweiterung, die die Skripte bereitstellt: Hook, Status und 3270-Befehle
/// `scripts` (Status) und `reload` (Neuladen, danach Status).
pub struct ScriptExtension {
    hooks: Arc<ScriptHooks>,
}

impl ScriptExtension {
    pub fn new(hooks: ScriptHooks) -> Self {
        Self {
            hooks: Arc::new(hooks),
        }
    }
}

impl Extension for ScriptExtension {
    fn hooks(&self) -> Vec<Arc<dyn Hook>> {
        vec![Arc::new(ScriptHook(self.hooks.clone()))]
    }

    fn status(&self) -> Option<(String, Value)> {
        Some(("scripts".into(), self.hooks.status()))
    }

    fn status_views(&self) -> Vec<(&'static str, crate::term3270::ViewFactory)> {
        let status = self.hooks.clone();
        let reload = self.hooks.clone();
        vec![
            (
                "scripts",
                section_view("Skripte", move || status.status()),
            ),
            (
                "reload",
                Box::new(move || {
                    reload.reload();
                    let view = section_view("Skripte (neu geladen)", {
                        let s = reload.clone();
                        move || s.status()
                    });
                    view()
                }),
            ),
        ]
    }
}
