//! Anbindung der Rhai-Skripte (`script-hooks`) an den Store-Hook und an die
//! Erweiterungen des Servers. Der Adapter uebersetzt Store-Operationen in
//! Werte fuer die Skripte und zurueck.

use std::sync::Arc;

use data_store::{DataRecord, EntityKey, StoreError};
use odata::query::ODataQuery;
use script_hooks::ScriptHooks;
use serde_json::{Value, json};

use crate::extension::{ActionError, ActionHandler, BoundAction, Extension, HeaderButton};
use crate::hook::{CurrentRecords, Extender, Hook, HookError, Operation};
use crate::term3270::section_view;

/// Bindet die Komponenten-Verwendungen der DDIC an die Skripte: je Verwendung ein Aufruf
/// des Komponenten-Skripts mit Feldzuordnung und Schrittweite. Fehlt ein Skript, bricht der
/// Start ab.
pub fn bind_components(
    hooks: &ScriptHooks,
    ddic: &spec::data_dictionary::DataDictionary,
) -> anyhow::Result<()> {
    let entities = ddic.entities();
    for usage in ddic.component_uses() {
        let component = ddic
            .components()
            .iter()
            .find(|c| c.name() == usage.component())
            .ok_or_else(|| {
                anyhow::anyhow!("verwendung {}: unbekannte Komponente", usage.instance())
            })?;
        let script = component
            .script()
            .ok_or_else(|| anyhow::anyhow!("komponente '{}' hat kein Skript", component.name()))?;
        let entity = entities
            .iter()
            .find(|e| e.name() == usage.entity())
            .ok_or_else(|| {
                anyhow::anyhow!("verwendung {}: unbekannte Entitaet", usage.instance())
            })?;
        let config = json!({
            "instance": usage.instance(),
            "fields": usage.fields(),
            "step_minutes": spec::component::effective_step(component, usage),
        });
        hooks
            .bind_component(&entity.setname(), script, config)
            .map_err(|e| {
                anyhow::anyhow!("verwendung {}.{}: {e}", usage.entity(), usage.instance())
            })?;
    }
    Ok(())
}

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
        self.0
            .before(set_name, op_name(op), value)
            .map_err(script_err)
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

    async fn before_patch(
        &self,
        store: &Extender,
        set_name: &str,
        key: &EntityKey,
        patch: &mut Value,
    ) -> Result<(), HookError> {
        let current = store.current_records(set_name, key).await?;
        let current = json!({
            "active": current.active.as_ref().map(record_to_value).transpose()?,
            "draft": current.draft.as_ref().map(record_to_value).transpose()?,
        });
        self.0
            .before_operation(
                set_name,
                op_name(Operation::Patch),
                patch,
                Some(key_value(key)),
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
        let mut v = record_to_value(value)?;
        if op == Operation::Read {
            self.0
                .after_read(set_name, "entity", None, None, &mut v)
                .map_err(script_err)?;
        } else {
            self.0
                .after(set_name, op_name(op), &mut v)
                .map_err(script_err)?;
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
    actions: Vec<BoundAction>,
}

impl ScriptExtension {
    pub fn new(hooks: ScriptHooks) -> Self {
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
                    chars
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
                        && !matches!(
                            action.name.as_str(),
                            "draftEdit"
                                | "draftActivate"
                                | "draftPrepare"
                                | "publishConfig"
                                | "stickyEdit"
                                | "stickySave"
                                | "stickyNew"
                                | "stickyDiscard"
                        ),
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
                    "{set}: Funktion action_{} fehlt",
                    action.name
                );
                self.actions.push(BoundAction {
                    set: set.clone(),
                    name: action.name.clone(),
                    returns: Some(entity.name().to_string()),
                    handler: Arc::new(ScriptAction {
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

struct ScriptAction {
    hooks: Arc<ScriptHooks>,
    set: String,
    name: String,
}

#[async_trait::async_trait]
impl ActionHandler for ScriptAction {
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
        let record = state
            .store
            .read_entity(&self.set, key, &ODataQuery::new())
            .await?;
        let record =
            serde_json::to_value(record).map_err(|e| ActionError::Failed(e.to_string()))?;
        let patch = self
            .hooks
            .action(&self.set, &self.name, record)
            .map_err(|e| ActionError::Failed(e.to_string()))?;
        let fields = patch
            .as_object()
            .ok_or_else(|| ActionError::Failed("Patch muss ein Objekt sein".into()))?;
        for field in fields.keys() {
            let writable = match entity.field_by_name(field) {
                Some(spec::FieldSpec::Atom {
                    computed,
                    immutable,
                    ..
                }) => !computed && !immutable,
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
        let updated = state
            .store
            .read_entity(&self.set, key, &ODataQuery::new())
            .await?;
        serde_json::to_value(updated).map_err(|e| ActionError::Failed(e.to_string()))
    }
}

impl Extension for ScriptExtension {
    fn actions(&self) -> Vec<BoundAction> {
        self.actions.clone()
    }

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
            ("scripts", section_view("Skripte", move || status.status())),
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
