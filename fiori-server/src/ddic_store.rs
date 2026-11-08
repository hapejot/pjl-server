//! Designer-Datastore: bildet DDIC-Elemente und ihre Konfigurations-Kindersets
//! auf `ddic.json` ab.
//!
//! Bewusst minimal. Keine Datenbank, keine Skripte, kein Entwurfszustand:
//! jede Schreiboperation wird gegen das Dictionary validiert und sofort
//! atomar in die Datei geschrieben. Die Oberflaeche sieht nur aktive Saetze.
//!
//! Lesen: `ddic.json` wird in Records projiziert.
//! Schreiben: die Records werden in das zuletzt gelesene JSON eingemischt.
//! Dabei bleiben alle Schluessel erhalten, die der Store nicht kennt
//! (z. B. `link`, `value_list`, `one_label`), damit nichts verloren geht.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use data_store::query::apply_query;
use data_store::{DataRecord, DataStore, DraftDataStore, EntityKey, Navigation, StoreError};
use odata::query::ODataQuery;
use serde_json::{Map, Value, json};
use spec::data_dictionary::DataDictionary;
use uuid::Uuid;

use crate::ddic_file;

/// Sets, die in `ddic.json` persistiert werden. Alle anderen Sets bleiben im Speicher.
const PERSISTED: [&str; 25] = [
    "PackageSet",
    "EntitySet",
    "FieldSet",
    "RelationSet",
    "AppSet",
    "ValueListSet",
    "ValueSet",
    "ComponentSet",
    "ComponentUseSet",
    "EntityListViewSet",
    "EntityListColumnSet",
    "EntityFilterDefaultSet",
    "EntitySelectionFieldSet",
    "EntityFacetSet",
    "EntityFacetFieldSet",
    "EntityHiddenFieldSet",
    "EntityHiddenFacetSet",
    "EntityUiSectionSet",
    "EntityUiListColumnSet",
    "EntityUiListTemplateSet",
    "FieldSideEffectSet",
    "ValueHelpFilterSet",
    "AppModuleSet",
    "AppControllerExtensionSet",
    "ComponentUseRoleSet",
];
const DEFAULT_PACKAGE: &str = "core";
/// Atom-Schluessel, die der Store als Records fuehrt (ausser `name`, `label`, `type`).
const ATOM_NUMBERS: [&str; 5] = ["max_length", "precision", "scale", "computed", "immutable"];

type Sets = HashMap<String, Vec<DataRecord>>;

#[derive(Clone)]
pub(crate) struct Snapshot {
    sets: Sets,
    /// Zuletzt geschriebenes (bzw. gelesenes) JSON; Grundlage fuer das Einmischen.
    base: Value,
    revision: u64,
}

pub struct DdicStore {
    path: PathBuf,
    inner: Mutex<Snapshot>,
    transient: bool,
    navigation: Navigation,
    navigations: Mutex<Vec<Navigation>>,
}

impl DdicStore {
    /// Oeffnet `ddic.json`. Fehlt die Datei, wird eine leere angelegt.
    pub fn open(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path = path.into();
        let base = if path.exists() {
            let text = std::fs::read_to_string(&path)?;
            serde_json::from_str::<Value>(&text)
                .map_err(|e| anyhow::anyhow!("'{}' is not valid JSON: {e}", path.display()))?
        } else {
            json!({"packages": [], "entities": [], "relationships": [], "apps": []})
        };
        // Pruefen, dass das Dictionary gueltig ist, bevor der Designer startet.
        serde_json::from_value::<DataDictionary>(base.clone())
            .map_err(|e| anyhow::anyhow!("'{}': {e}", path.display()))?;
        if !path.exists() {
            ddic_file::save(&path, &base)?;
        }
        let sets = project(&base);
        Ok(Self {
            path,
            inner: Mutex::new(Snapshot {
                sets,
                base,
                revision: 0,
            }),
            transient: false,
            navigation: Navigation::default(),
            navigations: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        self.inner.lock().expect("ddic lock").clone()
    }

    pub(crate) fn changed_since(&self, snapshot: &Snapshot) -> bool {
        self.inner.lock().expect("ddic lock").revision != snapshot.revision
    }

    pub(crate) fn fork(&self) -> Self {
        Self {
            path: self.path.clone(),
            inner: Mutex::new(self.snapshot()),
            transient: true,
            navigation: Navigation::default(),
            navigations: Mutex::new(self.navigations.lock().expect("nav lock").clone()),
        }
    }

    pub(crate) fn restore(&self, snapshot: Snapshot) {
        *self.inner.lock().expect("ddic lock") = snapshot;
    }

    pub(crate) fn save_session(
        &self,
        baseline: &Snapshot,
        working: &DdicStore,
    ) -> Result<(), StoreError> {
        let next = working.snapshot();
        let mut inner = self.inner.lock().expect("ddic lock");
        if inner.revision != baseline.revision {
            return Err(StoreError::Conflict(
                "Dictionary changed since editing started; discard and reload before editing again"
                    .into(),
            ));
        }
        let json = build(&inner.base, &next.sets)?;
        ddic_file::save(&self.path, &json)
            .map_err(|e| StoreError::Conflict(format!("cannot write ddic: {e:#}")))?;
        inner.sets = next.sets;
        inner.base = json;
        inner.revision += 1;
        Ok(())
    }

    fn read<T>(&self, f: impl FnOnce(&Sets) -> T) -> T {
        f(&self.inner.lock().expect("ddic lock").sets)
    }

    /// Schreibt in ein Set. Persistierte Sets werden vorher validiert und
    /// anschliessend atomar in die Datei geschrieben. Scheitert einer dieser
    /// Schritte, bleibt der Speicherstand unveraendert.
    fn write<T>(
        &self,
        set: &str,
        f: impl FnOnce(&mut Vec<DataRecord>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut inner = self.inner.lock().expect("ddic lock");
        let mut next = inner.sets.clone();
        let out = f(next.entry(set.to_string()).or_default())?;
        if !self.transient && PERSISTED.contains(&set) {
            let json = build(&inner.base, &next)?;
            ddic_file::save(&self.path, &json)
                .map_err(|e| StoreError::Conflict(format!("cannot write ddic: {e:#}")))?;
            inner.base = json;
        }
        inner.sets = next;
        inner.revision += 1;
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Projektion: ddic.json -> Records
// ---------------------------------------------------------------------------

fn id_for(set: &str, name: &str) -> String {
    Uuid::new_v5(&Uuid::NAMESPACE_OID, format!("{set}/{name}").as_bytes()).to_string()
}

fn items<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
}

fn push_record(sets: &mut Sets, set: &str, id: String, fields: Value) {
    sets.entry(set.to_string())
        .or_default()
        .push(rec(set, id, fields));
}

/// Paketname eines App-Eintrags: `"package": {"name": ...}` oder `"package": "..."`.
fn package_name(v: &Value) -> Option<&str> {
    match v.get("package")? {
        Value::String(s) => Some(s.as_str()),
        obj => text(obj, "name"),
    }
}

fn rec(set: &str, id: String, fields: Value) -> DataRecord {
    let mut obj = match fields {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    obj.insert("ID".into(), json!(id));
    obj.insert("IsActiveEntity".into(), json!(true));
    obj.retain(|_, v| !v.is_null());
    DataRecord::from_json(&Value::Object(obj))
        .unwrap_or_else(|e| panic!("projection of {set} failed: {e}"))
}

fn project(base: &Value) -> Sets {
    let mut sets: Sets = HashMap::new();
    for p in items(base, "packages") {
        if let Some(name) = text(p, "name") {
            push_record(
                &mut sets,
                "PackageSet",
                id_for("PackageSet", name),
                json!({ "name": name }),
            );
        }
    }
    for e in items(base, "entities") {
        let Some(name) = text(e, "name") else {
            continue;
        };
        let entity_id = id_for("EntitySet", name);
        let pkg = text(e, "package").unwrap_or(DEFAULT_PACKAGE);
        push_record(
            &mut sets,
            "EntitySet",
            entity_id.clone(),
            json!({
                "name": name,
                "plural": text(e, "type_name_plural"),
                "draft_enabled": e.get("draft_enabled").and_then(Value::as_bool).unwrap_or(true),
                "type_name": text(e, "type_name"),
                "audited": e.get("audited").and_then(Value::as_bool).unwrap_or(true),
                "package_ID": id_for("PackageSet", pkg),
            }),
        );

        for (view_order, view) in items(e, "list_views").iter().enumerate() {
            let Some(view_name) = text(view, "name") else {
                continue;
            };
            let view_id = id_for("EntityListViewSet", &format!("{name}/{view_name}"));
            push_record(
                &mut sets,
                "EntityListViewSet",
                view_id.clone(),
                json!({
                    "name": view_name,
                    "role": text(view, "role"),
                    "order": view_order,
                    "entity_ID": entity_id,
                }),
            );
            for (order, column) in items(view, "columns").iter().enumerate() {
                // Kurzform (Feldname als String) oder Objekt mit field/width/priority.
                let (field, width, priority) = match column.as_str() {
                    Some(field) => (Some(field), None, None),
                    None => (
                        text(column, "field"),
                        text(column, "width"),
                        text(column, "priority"),
                    ),
                };
                let Some(field) = field else { continue };
                push_record(
                    &mut sets,
                    "EntityListColumnSet",
                    id_for(
                        "EntityListColumnSet",
                        &format!("{name}/{view_name}/{order}"),
                    ),
                    json!({
                        "field": field,
                        "order": order,
                        "width": width,
                        "priority": priority,
                        "list_view_ID": view_id,
                    }),
                );
            }
        }
        if let Some(defaults) = e.get("filter_defaults").and_then(Value::as_object) {
            for (field, value) in defaults {
                let is_boolean = value.is_boolean();
                push_record(
                    &mut sets,
                    "EntityFilterDefaultSet",
                    id_for("EntityFilterDefaultSet", &format!("{name}/{field}")),
                    json!({
                        "field": field,
                        "value": if is_boolean { value.to_string() } else { value.as_str().unwrap_or_default().to_string() },
                        "is_boolean": is_boolean,
                        "entity_ID": entity_id,
                    }),
                );
            }
        }
        for (order, field) in items(e, "selection_fields").iter().enumerate() {
            let Some(field) = field.as_str() else {
                continue;
            };
            push_record(
                &mut sets,
                "EntitySelectionFieldSet",
                id_for("EntitySelectionFieldSet", &format!("{name}/{field}")),
                json!({ "field": field, "order": order, "entity_ID": entity_id }),
            );
        }
        for (facet_order, facet) in items(e, "facets").iter().enumerate() {
            let Some(facet_name) = text(facet, "facet") else {
                continue;
            };
            let facet_id = id_for("EntityFacetSet", &format!("{name}/{facet_name}"));
            push_record(
                &mut sets,
                "EntityFacetSet",
                facet_id.clone(),
                json!({
                    "facet": facet_name,
                    "view": text(facet, "view"),
                    "title": text(facet, "title"),
                    "order": facet_order,
                    "entity_ID": entity_id,
                }),
            );
            for (order, field) in items(facet, "fields").iter().enumerate() {
                let Some(field) = field.as_str() else {
                    continue;
                };
                push_record(
                    &mut sets,
                    "EntityFacetFieldSet",
                    id_for(
                        "EntityFacetFieldSet",
                        &format!("{name}/{facet_name}/{order}"),
                    ),
                    json!({ "field": field, "order": order, "facet_ID": facet_id }),
                );
            }
        }
        if let Some(ui) = e.get("ui") {
            for (order, field) in items(ui, "hidden_fields").iter().enumerate() {
                let Some(field) = field.as_str() else {
                    continue;
                };
                push_record(
                    &mut sets,
                    "EntityHiddenFieldSet",
                    id_for("EntityHiddenFieldSet", &format!("{name}/{field}")),
                    json!({ "field": field, "order": order, "entity_ID": entity_id }),
                );
            }
            for (order, facet) in items(ui, "hide_facets").iter().enumerate() {
                let Some(facet) = facet.as_str() else {
                    continue;
                };
                push_record(
                    &mut sets,
                    "EntityHiddenFacetSet",
                    id_for("EntityHiddenFacetSet", &format!("{name}/{facet}")),
                    json!({ "facet": facet, "order": order, "entity_ID": entity_id }),
                );
            }
            for (order, section) in items(ui, "sections").iter().enumerate() {
                let Some(key) = text(section, "key") else {
                    continue;
                };
                push_record(
                    &mut sets,
                    "EntityUiSectionSet",
                    id_for("EntityUiSectionSet", &format!("{name}/{key}")),
                    json!({
                        "key": key, "title": text(section, "title"),
                        "fragment": text(section, "fragment"), "anchor": text(section, "anchor"),
                        "placement": text(section, "placement"), "order": order, "entity_ID": entity_id,
                    }),
                );
            }
            for (order, column) in items(ui, "list_columns").iter().enumerate() {
                let Some(key) = text(column, "key") else {
                    continue;
                };
                push_record(
                    &mut sets,
                    "EntityUiListColumnSet",
                    id_for("EntityUiListColumnSet", &format!("{name}/{key}")),
                    json!({
                        "key": key, "header": text(column, "header"),
                        "fragment": text(column, "fragment"), "anchor": text(column, "anchor"),
                        "placement": text(column, "placement"), "width": text(column, "width"), "order": order,
                        "entity_ID": entity_id,
                    }),
                );
            }
            for (order, template) in items(ui, "list_templates").iter().enumerate() {
                let Some(key) = text(template, "key") else {
                    continue;
                };
                push_record(
                    &mut sets,
                    "EntityUiListTemplateSet",
                    id_for("EntityUiListTemplateSet", &format!("{name}/{key}")),
                    json!({
                        "key": key, "label": text(template, "label"),
                        "fragment": text(template, "fragment"), "order": order, "entity_ID": entity_id,
                    }),
                );
            }
        }

        for f in items(e, "fields") {
            let (kind, spec) = if let Some(atom) = f.get("Atom") {
                ("Atom", atom)
            } else if let Some(measure) = f.get("Measure") {
                ("Measure", measure)
            } else {
                continue;
            };
            let Some(fname) = text(spec, "name") else {
                continue;
            };
            let list_id = spec
                .get("value_list")
                .and_then(|vl| vl.get("FieldValueList"))
                .and_then(|fv| text(fv, "list_id"));
            let help = spec.get("value_list").and_then(Value::as_object);
            let entity_ref = help.and_then(|help| help.get("EntityRef"));
            let fixed_values = help.and_then(|help| help.get("FieldValueList"));
            let help_config = entity_ref.or(fixed_values);
            let field_id = id_for("FieldSet", &format!("{name}.{fname}"));
            let mut fields = json!({
                "name": fname,
                "label": text(spec, "label"),
                "type": text(spec, "edm_type").unwrap_or("Edm.Decimal"),
                "kind": kind,
                "package": text(spec, "package"),
                "entity_ID": entity_id,
                "value_list_ID": list_id,
                "value_help_type": if entity_ref.is_some() { "EntityRef" } else if fixed_values.is_some() { "FieldValueList" } else { "None" },
                "entity_set": entity_ref.and_then(|value| text(value, "entity_set")),
                "key_property": entity_ref.and_then(|value| text(value, "key_property")),
                "display_property": entity_ref.and_then(|value| text(value, "display_property")),
                "prefer_dialog": help_config.and_then(|value| value.get("prefer_dialog")).and_then(Value::as_bool).unwrap_or(false),
                "display_format": spec.pointer("/ui/format"),
                "link": spec.get("link").and_then(Value::as_bool).unwrap_or(false),
                "unit_field": text(spec, "unit_field"),
                "measure_kind": spec.get("kind").cloned().unwrap_or(Value::Null),
            });
            for key in ATOM_NUMBERS {
                fields[key] = spec.get(key).cloned().unwrap_or(Value::Null);
            }
            fields["multiline"] = json!(
                spec.get("multiline")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            );
            push_record(&mut sets, "FieldSet", field_id.clone(), fields);
            for (order, target) in items(spec, "side_effects").iter().enumerate() {
                let Some(target) = target.as_str() else {
                    continue;
                };
                push_record(
                    &mut sets,
                    "FieldSideEffectSet",
                    id_for("FieldSideEffectSet", &format!("{name}.{fname}/{order}")),
                    json!({ "target": target, "order": order, "field_ID": field_id }),
                );
            }
            if let Some(filters) = entity_ref
                .and_then(|value| value.get("filters"))
                .and_then(Value::as_array)
            {
                for (order, filter) in filters.iter().enumerate() {
                    push_record(
                        &mut sets,
                        "ValueHelpFilterSet",
                        id_for("ValueHelpFilterSet", &format!("{name}.{fname}/{order}")),
                        json!({
                            "local_property": text(filter, "local_property"),
                            "target_property": text(filter, "target_property"),
                            "order": order,
                            "field_ID": field_id,
                        }),
                    );
                }
            }
        }
    }
    for r in items(base, "relationships") {
        let Some(name) = text(r, "name") else {
            continue;
        };
        let one = text(r, "one").unwrap_or_default();
        let many = text(r, "many").unwrap_or_default();
        push_record(
            &mut sets,
            "RelationSet",
            id_for("RelationSet", name),
            json!({
                "name": name,
                "owned": r.get("owned").and_then(Value::as_bool).unwrap_or(false),
                "one_ID": id_for("EntitySet", one),
                "many_ID": id_for("EntitySet", many),
                "source_field": text(r, "one_field"),
                "target_field": text(r, "many_property"),
                "package": text(r, "package").unwrap_or(DEFAULT_PACKAGE),
                "one_label": text(r, "one_label"),
                "many_label": text(r, "many_label"),
            }),
        );
    }
    for l in items(base, "value_lists") {
        let (Some(id), Some(name)) = (text(l, "id"), text(l, "name")) else {
            continue;
        };
        let code_key = l.get("code_key").and_then(Value::as_bool).unwrap_or(false);
        push_record(
            &mut sets,
            "ValueListSet",
            id.to_string(),
            json!({ "name": name, "code_key": code_key }),
        );
        for v in items(l, "values") {
            let Some(code) = text(v, "code") else {
                continue;
            };
            push_record(
                &mut sets,
                "ValueSet",
                id_for("ValueSet", &format!("{id}/{code}")),
                json!({
                    "name": code,
                    "value": code,
                    "label": text(v, "label"),
                    "sort": v.get("sort").cloned().unwrap_or(Value::Null),
                    "list_ID": id,
                }),
            );
        }
    }
    for a in items(base, "apps") {
        let Some(name) = text(a, "name") else {
            continue;
        };
        let main = text(a, "main_entity").unwrap_or_default();
        let pkg = package_name(a).unwrap_or(DEFAULT_PACKAGE);
        let app_id = id_for("AppSet", name);
        push_record(
            &mut sets,
            "AppSet",
            app_id.clone(),
            json!({
                "name": name,
                "title": text(a, "title"),
                "subtitle": text(a, "subtitle"),
                "icon": text(a, "icon"),
                "main_entity_ID": id_for("EntitySet", main),
                "package_ID": id_for("PackageSet", pkg),
            }),
        );
        if let Some(ui) = a.get("ui") {
            for (order, module) in items(ui, "modules").iter().enumerate() {
                let Some(module) = module.as_str() else {
                    continue;
                };
                push_record(
                    &mut sets,
                    "AppModuleSet",
                    id_for("AppModuleSet", &format!("{name}/{module}")),
                    json!({ "module": module, "order": order, "app_ID": app_id }),
                );
            }
            for (order, extension) in items(ui, "controller_extensions").iter().enumerate() {
                let (Some(target), Some(controller)) =
                    (text(extension, "target"), text(extension, "controller"))
                else {
                    continue;
                };
                push_record(
                    &mut sets,
                    "AppControllerExtensionSet",
                    id_for("AppControllerExtensionSet", &format!("{name}/{target}")),
                    json!({ "target": target, "controller": controller, "order": order, "app_ID": app_id }),
                );
            }
        }
    }
    for component in items(base, "components") {
        let Some(name) = text(component, "name") else {
            continue;
        };
        push_record(
            &mut sets,
            "ComponentSet",
            id_for("ComponentSet", name),
            json!({
                "name": name,
                "kind": component.get("kind"),
                "script": text(component, "script"),
                "unit": component.get("unit"),
                "step_minutes": component.get("step_minutes"),
            }),
        );
    }
    for usage in items(base, "component_uses") {
        let (Some(entity), Some(instance)) = (text(usage, "entity"), text(usage, "instance"))
        else {
            continue;
        };
        let use_id = id_for("ComponentUseSet", &format!("{entity}/{instance}"));
        push_record(
            &mut sets,
            "ComponentUseSet",
            use_id.clone(),
            json!({
                "entity_ID": id_for("EntitySet", entity),
                "component_ID": id_for("ComponentSet", text(usage, "component").unwrap_or_default()),
                "instance": instance,
                "step_minutes": usage.get("step_minutes"),
                "section_title": text(usage, "section_title"),
            }),
        );
        let fields = usage.get("fields").and_then(Value::as_object);
        let labels = usage.get("labels").and_then(Value::as_object);
        let mut roles = std::collections::BTreeSet::new();
        roles.extend(fields.into_iter().flat_map(|map| map.keys().cloned()));
        roles.extend(labels.into_iter().flat_map(|map| map.keys().cloned()));
        for (order, role) in roles.into_iter().enumerate() {
            push_record(
                &mut sets,
                "ComponentUseRoleSet",
                id_for(
                    "ComponentUseRoleSet",
                    &format!("{entity}/{instance}/{role}"),
                ),
                json!({
                    "role": role,
                    "field": fields.and_then(|map| map.get(&role)),
                    "label": labels.and_then(|map| map.get(&role)),
                    "order": order,
                    "component_use_ID": use_id,
                }),
            );
        }
    }
    sets
}

// ---------------------------------------------------------------------------
// Rueckfuehrung: Records -> ddic.json (Einmischen in das Basis-JSON)
// ---------------------------------------------------------------------------

fn value_of(r: &DataRecord) -> Value {
    serde_json::to_value(r).unwrap_or(Value::Null)
}

/// Original-Objekt mit `name` aus einer Liste des Basis-JSON.
fn original<'a>(base: &'a Value, key: &str, name: &str) -> Option<&'a Value> {
    items(base, key)
        .iter()
        .find(|o| text(o, "name") == Some(name))
}

/// Setzt `key` auf `v`, oder entfernt ihn bei `null`.
fn put(obj: &mut Map<String, Value>, key: &str, v: &Value) {
    if v.is_null() {
        obj.remove(key);
    } else {
        obj.insert(key.into(), v.clone());
    }
}

fn names_by_id(sets: &Sets, set: &str) -> HashMap<String, String> {
    sets.get(set)
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let v = value_of(r);
            Some((text(&v, "ID")?.to_string(), text(&v, "name")?.to_string()))
        })
        .collect()
}

fn child_records<'a>(records: &'a [Value], parent_key: &str, parent_id: &str) -> Vec<&'a Value> {
    let mut children: Vec<_> = records
        .iter()
        .filter(|record| text(record, parent_key) == Some(parent_id))
        .collect();
    children.sort_by_key(|record| {
        record
            .get("order")
            .and_then(Value::as_u64)
            .unwrap_or_default()
    });
    children
}

fn set_array(obj: &mut Map<String, Value>, key: &str, values: Vec<Value>) {
    if values.is_empty() {
        obj.remove(key);
    } else {
        obj.insert(key.into(), Value::Array(values));
    }
}

fn build(base: &Value, sets: &Sets) -> Result<Value, StoreError> {
    let empty = Vec::new();
    let recs = |set: &str| {
        sets.get(set)
            .unwrap_or(&empty)
            .iter()
            .map(value_of)
            .collect::<Vec<_>>()
    };
    let pkg_names = names_by_id(sets, "PackageSet");
    let ent_names = names_by_id(sets, "EntitySet");
    let component_names = names_by_id(sets, "ComponentSet");
    let need = |v: &Value| {
        text(v, "name")
            .map(str::to_string)
            .ok_or_else(|| StoreError::Conflict("name fehlt".into()))
    };

    let mut packages = Vec::new();
    for r in recs("PackageSet") {
        let name = need(&r)?;
        let mut obj = original(base, "packages", &name)
            .and_then(|o| o.as_object().cloned())
            .unwrap_or_default();
        obj.insert("name".into(), json!(name));
        packages.push(Value::Object(obj));
    }

    let mut entities = Vec::new();
    let field_recs = recs("FieldSet");
    let side_effect_recs = recs("FieldSideEffectSet");
    let value_help_filter_recs = recs("ValueHelpFilterSet");
    let list_view_recs = recs("EntityListViewSet");
    let list_column_recs = recs("EntityListColumnSet");
    let filter_default_recs = recs("EntityFilterDefaultSet");
    let selection_field_recs = recs("EntitySelectionFieldSet");
    let facet_recs = recs("EntityFacetSet");
    let facet_field_recs = recs("EntityFacetFieldSet");
    let hidden_field_recs = recs("EntityHiddenFieldSet");
    let hidden_facet_recs = recs("EntityHiddenFacetSet");
    let ui_section_recs = recs("EntityUiSectionSet");
    let ui_column_recs = recs("EntityUiListColumnSet");
    let ui_template_recs = recs("EntityUiListTemplateSet");
    for r in recs("EntitySet") {
        let name = need(&r)?;
        let ent_id = text(&r, "ID").unwrap_or_default().to_string();
        let mut obj = original(base, "entities", &name)
            .and_then(|o| o.as_object().cloned())
            .unwrap_or_default();
        let pkg = r
            .get("package_ID")
            .and_then(Value::as_str)
            .and_then(|id| pkg_names.get(id))
            .ok_or_else(|| {
                StoreError::Conflict(format!("Entitaet '{name}': Paket fehlt oder unbekannt"))
            })?;
        obj.insert("name".into(), json!(name));
        obj.insert("package".into(), json!(pkg));
        let plural = text(&r, "plural")
            .map(str::to_string)
            .unwrap_or_else(|| format!("{name}s"));
        obj.insert("type_name_plural".into(), json!(plural));
        match r.get("draft_enabled") {
            None | Some(Value::Bool(true)) => {
                obj.remove("draft_enabled");
            }
            Some(Value::Bool(false)) => {
                obj.insert("draft_enabled".into(), json!(false));
            }
            Some(_) => {
                return Err(StoreError::Conflict(
                    "draft_enabled muss Boolean sein".into(),
                ));
            }
        }
        put(
            &mut obj,
            "type_name",
            &r.get("type_name").cloned().unwrap_or(Value::Null),
        );
        match r.get("audited").and_then(Value::as_bool).unwrap_or(true) {
            true => {
                obj.remove("audited");
            }
            false => {
                obj.insert("audited".into(), json!(false));
            }
        }
        let list_views: Vec<Value> = child_records(&list_view_recs, "entity_ID", &ent_id)
            .into_iter()
            .map(|view| {
                let view_id = text(view, "ID").unwrap_or_default();
                let columns: Vec<Value> = child_records(&list_column_recs, "list_view_ID", view_id)
                    .into_iter()
                    .filter_map(|column| {
                        let field = text(column, "field")?;
                        let mut value = json!({
                            "field": field,
                            "width": text(column, "width"),
                            "priority": text(column, "priority"),
                        });
                        if let Some(fields) = value.as_object_mut() {
                            fields.retain(|_, value| !value.is_null());
                        }
                        Some(value)
                    })
                    .collect();
                let mut value = json!({
                    "name": text(view, "name"),
                    "role": text(view, "role"),
                    "columns": columns,
                });
                if let Some(fields) = value.as_object_mut() {
                    fields.retain(|_, value| !value.is_null());
                }
                value
            })
            .collect();
        set_array(&mut obj, "list_views", list_views);

        let mut filter_defaults = Map::new();
        for filter in child_records(&filter_default_recs, "entity_ID", &ent_id) {
            let Some(field) = text(filter, "field") else {
                continue;
            };
            let value = text(filter, "value").unwrap_or_default();
            let value = if filter
                .get("is_boolean")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                match value.parse::<bool>() {
                    Ok(value) => json!(value),
                    Err(_) => {
                        return Err(StoreError::Conflict(format!(
                            "Filterdefault '{field}' muss true oder false sein"
                        )));
                    }
                }
            } else {
                json!(value)
            };
            filter_defaults.insert(field.into(), value);
        }
        if filter_defaults.is_empty() {
            obj.remove("filter_defaults");
        } else {
            obj.insert("filter_defaults".into(), Value::Object(filter_defaults));
        }

        let selection_fields: Vec<Value> =
            child_records(&selection_field_recs, "entity_ID", &ent_id)
                .into_iter()
                .filter_map(|field| text(field, "field").map(|field| json!(field)))
                .collect();
        set_array(&mut obj, "selection_fields", selection_fields);

        let facets: Vec<Value> = child_records(&facet_recs, "entity_ID", &ent_id)
            .into_iter()
            .map(|facet| {
                let facet_id = text(facet, "ID").unwrap_or_default();
                let fields: Vec<Value> = child_records(&facet_field_recs, "facet_ID", facet_id)
                    .into_iter()
                    .filter_map(|field| text(field, "field").map(|field| json!(field)))
                    .collect();
                let mut value = json!({
                    "facet": text(facet, "facet"),
                    "view": text(facet, "view"),
                    "title": text(facet, "title"),
                    "fields": fields,
                });
                if let Some(fields) = value.as_object_mut() {
                    fields.retain(|_, value| !value.is_null());
                }
                value
            })
            .collect();
        set_array(&mut obj, "facets", facets);

        let hidden_fields: Vec<Value> = child_records(&hidden_field_recs, "entity_ID", &ent_id)
            .into_iter()
            .filter_map(|field| text(field, "field").map(|field| json!(field)))
            .collect();
        let hide_facets: Vec<Value> = child_records(&hidden_facet_recs, "entity_ID", &ent_id)
            .into_iter()
            .filter_map(|facet| text(facet, "facet").map(|facet| json!(facet)))
            .collect();
        let sections: Vec<Value> = child_records(&ui_section_recs, "entity_ID", &ent_id)
            .into_iter()
            .map(|section| {
                json!({
                    "key": text(section, "key"), "title": text(section, "title"),
                    "fragment": text(section, "fragment"), "anchor": text(section, "anchor"),
                    "placement": text(section, "placement"),
                })
            })
            .collect();
        let list_columns: Vec<Value> = child_records(&ui_column_recs, "entity_ID", &ent_id)
            .into_iter()
            .map(|column| {
                json!({
                    "key": text(column, "key"), "header": text(column, "header"),
                    "fragment": text(column, "fragment"), "anchor": text(column, "anchor"),
                    "placement": text(column, "placement"), "width": text(column, "width"),
                })
            })
            .collect();
        let list_templates: Vec<Value> = child_records(&ui_template_recs, "entity_ID", &ent_id)
            .into_iter()
            .map(|template| {
                json!({
                    "key": text(template, "key"), "label": text(template, "label"),
                    "fragment": text(template, "fragment"),
                })
            })
            .collect();
        if hidden_fields.is_empty()
            && hide_facets.is_empty()
            && sections.is_empty()
            && list_columns.is_empty()
            && list_templates.is_empty()
        {
            obj.remove("ui");
        } else {
            let mut ui = Map::new();
            for (key, values) in [
                ("hidden_fields", hidden_fields),
                ("hide_facets", hide_facets),
                ("sections", sections),
                ("list_columns", list_columns),
                ("list_templates", list_templates),
            ] {
                if !values.is_empty() {
                    ui.insert(key.into(), Value::Array(values));
                }
            }
            obj.insert("ui".into(), Value::Object(ui));
        }

        // Nicht direkt bearbeitete Felder (z. B. Navigations-Lookups) bleiben erhalten.
        let orig_fields = obj
            .get("fields")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut fields: Vec<Value> = orig_fields
            .iter()
            .filter(|f| f.get("Atom").is_none() && f.get("Measure").is_none())
            .cloned()
            .collect();
        for fr in field_recs
            .iter()
            .filter(|f| text(f, "entity_ID") == Some(ent_id.as_str()))
        {
            let fname = need(fr)?;
            let variant = if text(fr, "kind") == Some("Measure") {
                "Measure"
            } else {
                "Atom"
            };
            let mut spec = orig_fields
                .iter()
                .find_map(|f| {
                    f.get(variant)
                        .filter(|field| text(field, "name") == Some(fname.as_str()))
                })
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            spec.insert("name".into(), json!(fname));
            spec.insert("label".into(), json!(text(fr, "label").unwrap_or(&fname)));
            put(
                &mut spec,
                "package",
                &fr.get("package").cloned().unwrap_or(Value::Null),
            );
            if variant == "Measure" {
                put(
                    &mut spec,
                    "precision",
                    &fr.get("precision").cloned().unwrap_or(Value::Null),
                );
                put(
                    &mut spec,
                    "scale",
                    &fr.get("scale").cloned().unwrap_or(Value::Null),
                );
                put(
                    &mut spec,
                    "unit_field",
                    &fr.get("unit_field").cloned().unwrap_or(Value::Null),
                );
                put(
                    &mut spec,
                    "kind",
                    &fr.get("measure_kind").cloned().unwrap_or(Value::Null),
                );
                fields.push(json!({ "Measure": spec }));
                continue;
            }
            spec.insert(
                "edm_type".into(),
                json!(text(fr, "type").unwrap_or("Edm.String")),
            );
            for key in ATOM_NUMBERS {
                put(&mut spec, key, &fr.get(key).cloned().unwrap_or(Value::Null));
            }
            if fr
                .get("multiline")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                spec.insert("multiline".into(), json!(true));
            } else {
                spec.remove("multiline");
            }
            if fr.get("link").and_then(Value::as_bool).unwrap_or(false) {
                spec.insert("link".into(), json!(true));
            } else {
                spec.remove("link");
            }
            let field_id = text(&fr, "ID").unwrap_or_default();
            let side_effects: Vec<Value> = child_records(&side_effect_recs, "field_ID", field_id)
                .into_iter()
                .filter_map(|effect| text(effect, "target").map(|target| json!(target)))
                .collect();
            if side_effects.is_empty() {
                spec.remove("side_effects");
            } else {
                spec.insert("side_effects".into(), Value::Array(side_effects));
            }
            if let Some(format) = text(&fr, "display_format") {
                spec.insert("ui".into(), json!({ "format": format }));
            } else {
                spec.remove("ui");
            }
            let prefer_dialog = fr
                .get("prefer_dialog")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match text(&fr, "value_help_type").unwrap_or("None") {
                "None" => {
                    spec.remove("value_list");
                }
                "FieldValueList" => match text(&fr, "value_list_ID") {
                    Some(list_id) => {
                        spec.insert(
                            "value_list".into(),
                            json!({ "FieldValueList": {
                                "list_id": list_id,
                                "prefer_dialog": prefer_dialog,
                            }}),
                        );
                    }
                    None => {
                        spec.remove("value_list");
                    }
                },
                "EntityRef" => {
                    let filters: Vec<Value> =
                        child_records(&value_help_filter_recs, "field_ID", field_id)
                            .into_iter()
                            .map(|filter| {
                                let local = text(filter, "local_property").unwrap_or_default();
                                let target = text(filter, "target_property").unwrap_or_default();
                                json!({ "local_property": local, "target_property": target })
                            })
                            .collect();
                    let mut config = json!({
                        "entity_set": text(&fr, "entity_set"),
                        "key_property": text(&fr, "key_property").unwrap_or("ID"),
                        "display_property": text(&fr, "display_property"),
                        "filters": filters,
                        "prefer_dialog": prefer_dialog,
                    });
                    if let Some(config) = config.as_object_mut() {
                        config.retain(|_, value| !value.is_null());
                    }
                    spec.insert("value_list".into(), json!({ "EntityRef": config }));
                }
                other => {
                    return Err(StoreError::Conflict(format!(
                        "Feld '{}': unbekannter Wertehilfetyp '{other}'",
                        fname
                    )));
                }
            }
            fields.push(json!({ "Atom": spec }));
        }
        obj.insert("fields".into(), Value::Array(fields));
        entities.push(Value::Object(obj));
    }

    let mut relationships = Vec::new();
    for r in recs("RelationSet") {
        let name = need(&r)?;
        let one = r
            .get("one_ID")
            .and_then(Value::as_str)
            .and_then(|id| ent_names.get(id));
        let many = r
            .get("many_ID")
            .and_then(Value::as_str)
            .and_then(|id| ent_names.get(id));
        // Beziehung ohne bekannte Entitaeten (z. B. nach Loeschen) entfaellt.
        let (Some(one), Some(many)) = (one, many) else {
            continue;
        };
        let mut obj = original(base, "relationships", &name)
            .and_then(|o| o.as_object().cloned())
            .unwrap_or_default();
        obj.insert("name".into(), json!(name));
        obj.insert("one".into(), json!(one));
        obj.insert("many".into(), json!(many));
        put(
            &mut obj,
            "one_field",
            &r.get("source_field").cloned().unwrap_or(Value::Null),
        );
        put(
            &mut obj,
            "many_property",
            &r.get("target_field").cloned().unwrap_or(Value::Null),
        );
        put(
            &mut obj,
            "package",
            &r.get("package").cloned().unwrap_or(Value::Null),
        );
        put(
            &mut obj,
            "one_label",
            &r.get("one_label").cloned().unwrap_or(Value::Null),
        );
        put(
            &mut obj,
            "many_label",
            &r.get("many_label").cloned().unwrap_or(Value::Null),
        );
        if r.get("owned").and_then(Value::as_bool).unwrap_or(false) {
            obj.insert("owned".into(), json!(true));
        } else {
            obj.remove("owned");
        }
        obj.entry("package").or_insert(json!(DEFAULT_PACKAGE));
        relationships.push(Value::Object(obj));
    }

    let mut apps = Vec::new();
    let app_module_recs = recs("AppModuleSet");
    let app_extension_recs = recs("AppControllerExtensionSet");
    for r in recs("AppSet") {
        let name = need(&r)?;
        let main = r
            .get("main_entity_ID")
            .and_then(Value::as_str)
            .and_then(|id| ent_names.get(id));
        let Some(main) = main else {
            return Err(StoreError::Conflict(format!(
                "App '{name}': Hauptentitaet fehlt"
            )));
        };
        let pkg = r
            .get("package_ID")
            .and_then(Value::as_str)
            .and_then(|id| pkg_names.get(id))
            .ok_or_else(|| {
                StoreError::Conflict(format!("App '{name}': Paket fehlt oder unbekannt"))
            })?;
        let mut obj = original(base, "apps", &name)
            .and_then(|o| o.as_object().cloned())
            .unwrap_or_default();
        obj.insert("name".into(), json!(name));
        obj.insert("title".into(), json!(text(&r, "title").unwrap_or(&name)));
        obj.insert("main_entity".into(), json!(main));
        put(
            &mut obj,
            "subtitle",
            &r.get("subtitle").cloned().unwrap_or(Value::Null),
        );
        put(
            &mut obj,
            "icon",
            &r.get("icon").cloned().unwrap_or(Value::Null),
        );
        obj.insert("package".into(), json!({ "name": pkg }));
        let app_id = text(&r, "ID").unwrap_or_default();
        let modules: Vec<Value> = child_records(&app_module_recs, "app_ID", app_id)
            .into_iter()
            .filter_map(|module| text(module, "module").map(|module| json!(module)))
            .collect();
        let controller_extensions: Vec<Value> =
            child_records(&app_extension_recs, "app_ID", app_id)
                .into_iter()
                .map(|extension| {
                    json!({
                        "target": text(extension, "target"),
                        "controller": text(extension, "controller"),
                    })
                })
                .collect();
        if modules.is_empty() && controller_extensions.is_empty() {
            obj.remove("ui");
        } else {
            let mut ui = Map::new();
            if !modules.is_empty() {
                ui.insert("modules".into(), Value::Array(modules));
            }
            if !controller_extensions.is_empty() {
                ui.insert(
                    "controller_extensions".into(),
                    Value::Array(controller_extensions),
                );
            }
            obj.insert("ui".into(), Value::Object(ui));
        }
        apps.push(Value::Object(obj));
    }

    let mut components = Vec::new();
    for r in recs("ComponentSet") {
        let name = need(&r)?;
        let mut obj = original(base, "components", &name)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        obj.insert("name".into(), json!(name));
        put(
            &mut obj,
            "kind",
            &r.get("kind").cloned().unwrap_or(Value::Null),
        );
        put(
            &mut obj,
            "script",
            &r.get("script").cloned().unwrap_or(Value::Null),
        );
        put(
            &mut obj,
            "unit",
            &r.get("unit").cloned().unwrap_or(Value::Null),
        );
        put(
            &mut obj,
            "step_minutes",
            &r.get("step_minutes").cloned().unwrap_or(Value::Null),
        );
        components.push(Value::Object(obj));
    }

    let mut component_uses = Vec::new();
    let component_role_recs = recs("ComponentUseRoleSet");
    for r in recs("ComponentUseSet") {
        let entity = r
            .get("entity_ID")
            .and_then(Value::as_str)
            .and_then(|id| ent_names.get(id))
            .ok_or_else(|| StoreError::Conflict("ComponentUse: entity fehlt".into()))?;
        let component = r
            .get("component_ID")
            .and_then(Value::as_str)
            .and_then(|id| component_names.get(id))
            .ok_or_else(|| StoreError::Conflict("ComponentUse: Komponente fehlt".into()))?;
        let instance = text(&r, "instance")
            .ok_or_else(|| StoreError::Conflict("ComponentUse: instance fehlt".into()))?;
        let mut obj = original(base, "component_uses", &instance)
            .filter(|usage| text(usage, "entity") == Some(entity))
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        obj.insert("entity".into(), json!(entity));
        obj.insert("instance".into(), json!(instance));
        obj.insert("component".into(), json!(component));
        let use_id = text(&r, "ID").unwrap_or_default();
        let mut fields = Map::new();
        let mut labels = Map::new();
        for role in child_records(&component_role_recs, "component_use_ID", use_id) {
            let Some(name) = text(role, "role") else {
                continue;
            };
            if let Some(field) = text(role, "field") {
                fields.insert(name.into(), json!(field));
            }
            if let Some(label) = text(role, "label") {
                labels.insert(name.into(), json!(label));
            }
        }
        if fields.is_empty() {
            obj.remove("fields");
        } else {
            obj.insert("fields".into(), Value::Object(fields));
        }
        if labels.is_empty() {
            obj.remove("labels");
        } else {
            obj.insert("labels".into(), Value::Object(labels));
        }
        put(
            &mut obj,
            "step_minutes",
            &r.get("step_minutes").cloned().unwrap_or(Value::Null),
        );
        put(
            &mut obj,
            "section_title",
            &r.get("section_title").cloned().unwrap_or(Value::Null),
        );
        component_uses.push(Value::Object(obj));
    }

    let mut value_lists = Vec::new();
    let value_recs = recs("ValueSet");
    for r in recs("ValueListSet") {
        let name = need(&r)?;
        let id = text(&r, "ID").unwrap_or_default().to_string();
        let values: Vec<Value> = value_recs
            .iter()
            .filter(|v| text(v, "list_ID") == Some(id.as_str()))
            .filter_map(|v| {
                let code = text(v, "value")?;
                Some(json!({
                    "code": code,
                    "label": text(v, "label").unwrap_or(code),
                    "sort": v.get("sort").and_then(Value::as_u64).unwrap_or(0),
                }))
            })
            .collect();
        let mut obj = Map::new();
        obj.insert("id".into(), json!(id));
        obj.insert("name".into(), json!(name));
        if r.get("code_key") == Some(&json!(true)) {
            obj.insert("code_key".into(), json!(true));
        }
        if !values.is_empty() {
            obj.insert("values".into(), Value::Array(values));
        }
        value_lists.push(Value::Object(obj));
    }

    let mut out = base.as_object().cloned().unwrap_or_default();
    if value_lists.is_empty() {
        out.remove("value_lists");
    } else {
        out.insert("value_lists".into(), Value::Array(value_lists));
    }
    out.insert("packages".into(), Value::Array(packages));
    out.insert("entities".into(), Value::Array(entities));
    out.insert("relationships".into(), Value::Array(relationships));
    out.insert("apps".into(), Value::Array(apps));
    if components.is_empty() {
        out.remove("components");
    } else {
        out.insert("components".into(), Value::Array(components));
    }
    if component_uses.is_empty() {
        out.remove("component_uses");
    } else {
        out.insert("component_uses".into(), Value::Array(component_uses));
    }
    let out = Value::Object(out);
    let mut dictionary = serde_json::from_value::<DataDictionary>(out.clone())
        .map_err(|e| StoreError::Conflict(format!("ungueltiges Dictionary: {e}")))?;
    dictionary
        .expand_components()
        .map_err(|e| StoreError::Conflict(format!("ungueltige Komponenten: {e}")))?;
    dictionary
        .apply_value_list_lookups()
        .map_err(|e| StoreError::Conflict(format!("ungueltige Wertelisten: {e}")))?;
    dictionary
        .validate_list_views()
        .map_err(|e| StoreError::Conflict(format!("ungueltige Listenansichten: {e}")))?;
    dictionary
        .validate_primary_keys()
        .map_err(|e| StoreError::Conflict(format!("ungueltiger Primaerschluessel: {e}")))?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Datastore-Schnittstelle
// ---------------------------------------------------------------------------

/// Schluessel fuer die Eindeutigkeit innerhalb eines Sets.
fn unique_key(set: &str, v: &Value) -> Option<String> {
    let parent_key = match set {
        "ComponentUseSet"
        | "EntityListViewSet"
        | "EntityFilterDefaultSet"
        | "EntitySelectionFieldSet"
        | "EntityFacetSet"
        | "EntityHiddenFieldSet"
        | "EntityHiddenFacetSet"
        | "EntityUiSectionSet"
        | "EntityUiListColumnSet"
        | "EntityUiListTemplateSet" => "entity_ID",
        "EntityListColumnSet" => "list_view_ID",
        "EntityFacetFieldSet" => "facet_ID",
        "FieldSideEffectSet" | "ValueHelpFilterSet" => "field_ID",
        "AppModuleSet" | "AppControllerExtensionSet" => "app_ID",
        "ComponentUseRoleSet" => "component_use_ID",
        _ => "",
    };
    if !parent_key.is_empty() {
        let parent = text(v, parent_key)?;
        let child = match set {
            "ComponentUseSet" => text(v, "instance")?,
            "EntityListViewSet" => text(v, "name")?,
            "EntityFilterDefaultSet" | "EntitySelectionFieldSet" | "EntityHiddenFieldSet" => {
                text(v, "field")?
            }
            "EntityFacetSet" | "EntityHiddenFacetSet" => text(v, "facet")?,
            "EntityUiSectionSet" | "EntityUiListColumnSet" | "EntityUiListTemplateSet" => {
                text(v, "key")?
            }
            "EntityListColumnSet" | "EntityFacetFieldSet" => text(v, "field")?,
            "FieldSideEffectSet" => text(v, "target")?,
            "ValueHelpFilterSet" => {
                return Some(format!(
                    "{parent}/{}:{}",
                    text(v, "local_property")?,
                    text(v, "target_property")?
                ));
            }
            "AppModuleSet" => text(v, "module")?,
            "AppControllerExtensionSet" => text(v, "target")?,
            "ComponentUseRoleSet" => text(v, "role")?,
            _ => return None,
        };
        return Some(format!("{parent}/{child}"));
    }
    let name = if set == "ValueSet" {
        text(v, "value")?
    } else {
        text(v, "name")?
    };
    Some(match set {
        "FieldSet" => format!("{}/{name}", text(v, "entity_ID").unwrap_or_default()),
        "ValueSet" => format!(
            "{}/{}",
            text(v, "list_ID").unwrap_or_default(),
            text(v, "value").unwrap_or_default()
        ),
        _ => name.to_string(),
    })
}

fn cascade_children(sets: &mut Sets, set: &str, id: &str) {
    const CASCADES: [(&str, &str, &str); 20] = [
        ("EntitySet", "FieldSet", "entity_ID"),
        ("EntitySet", "EntityListViewSet", "entity_ID"),
        ("EntitySet", "EntityFilterDefaultSet", "entity_ID"),
        ("EntitySet", "EntitySelectionFieldSet", "entity_ID"),
        ("EntitySet", "EntityFacetSet", "entity_ID"),
        ("EntitySet", "EntityHiddenFieldSet", "entity_ID"),
        ("EntitySet", "EntityHiddenFacetSet", "entity_ID"),
        ("EntitySet", "EntityUiSectionSet", "entity_ID"),
        ("EntitySet", "EntityUiListColumnSet", "entity_ID"),
        ("EntitySet", "EntityUiListTemplateSet", "entity_ID"),
        ("EntitySet", "ComponentUseSet", "entity_ID"),
        ("EntitySet", "RelationSet", "one_ID"),
        ("EntitySet", "RelationSet", "many_ID"),
        ("EntityListViewSet", "EntityListColumnSet", "list_view_ID"),
        ("EntityFacetSet", "EntityFacetFieldSet", "facet_ID"),
        ("FieldSet", "FieldSideEffectSet", "field_ID"),
        ("FieldSet", "ValueHelpFilterSet", "field_ID"),
        ("AppSet", "AppModuleSet", "app_ID"),
        ("AppSet", "AppControllerExtensionSet", "app_ID"),
        ("ComponentUseSet", "ComponentUseRoleSet", "component_use_ID"),
    ];
    const COMPONENT_CASCADE: (&str, &str, &str) =
        ("ComponentSet", "ComponentUseSet", "component_ID");
    let mut pending = vec![(set.to_string(), id.to_string())];
    while let Some((parent_set, parent_id)) = pending.pop() {
        for (_, child_set, foreign_key) in CASCADES
            .into_iter()
            .chain(std::iter::once(COMPONENT_CASCADE))
            .filter(|(relation_parent, _, _)| *relation_parent == parent_set.as_str())
        {
            let removed: Vec<String> = sets
                .get(child_set)
                .into_iter()
                .flatten()
                .filter(|record| {
                    record
                        .get(foreign_key)
                        .and_then(|value| value.as_str().map(str::to_string))
                        .as_deref()
                        == Some(parent_id.as_str())
                })
                .map(|record| record.id().to_string())
                .collect();
            if let Some(children) = sets.get_mut(child_set) {
                children.retain(|record| {
                    record
                        .get(foreign_key)
                        .and_then(|value| value.as_str().map(str::to_string))
                        .as_deref()
                        != Some(parent_id.as_str())
                });
            }
            pending.extend(
                removed
                    .into_iter()
                    .map(|child_id| (child_set.to_string(), child_id)),
            );
        }
    }
}

fn not_found(set: &str, key: &EntityKey) -> StoreError {
    StoreError::not_found(format!("{set}/{}", key.id))
}

#[async_trait::async_trait]
impl DataStore for DdicStore {
    fn component_info(&self) -> String {
        format!("DdicStore -> {}", self.path.display())
    }

    async fn get_collection(
        &self,
        set: &str,
        query: &ODataQuery,
    ) -> Result<Vec<DataRecord>, StoreError> {
        let mut records = self.read(|s| s.get(set).cloned().unwrap_or_default());
        apply_query(&mut records, query);
        Ok(records)
    }

    async fn count(&self, set: &str, query: &ODataQuery) -> usize {
        let mut q = query.clone();
        q.top = None;
        q.skip = None;
        let mut records = self.read(|s| s.get(set).cloned().unwrap_or_default());
        apply_query(&mut records, &q);
        records.len()
    }

    async fn read_entity(
        &self,
        set: &str,
        key: &EntityKey,
        _query: &ODataQuery,
    ) -> Result<DataRecord, StoreError> {
        self.read(|s| {
            s.get(set)
                .and_then(|list| list.iter().find(|r| r.id() == key.id).cloned())
        })
        .ok_or_else(|| not_found(set, key))
    }

    async fn create_entity(&self, set: &str, data: &Value) -> Result<DataRecord, StoreError> {
        let mut v = data.clone();
        let obj = v
            .as_object_mut()
            .ok_or_else(|| StoreError::Serialize("Objekt erwartet".into()))?;
        if set == "EntitySet" {
            obj.entry("draft_enabled").or_insert(json!(true));
        }
        // Festwerte tragen den Code zusaetzlich in `name` (Titel der Liste).
        if set == "ValueSet" && !obj.contains_key("name") {
            if let Some(code) = obj.get("value").cloned() {
                obj.insert("name".into(), code);
            }
        }
        obj.remove("IsActiveEntity");
        obj.insert("IsActiveEntity".into(), json!(true));
        let new = DataRecord::from_json(&v)?;
        self.write(set, |list| {
            let new_value = value_of(&new);
            if let Some(key) = unique_key(set, &new_value) {
                let dup = list
                    .iter()
                    .any(|r| unique_key(set, &value_of(r)).as_deref() == Some(key.as_str()));
                if dup {
                    return Err(StoreError::Conflict(format!(
                        "{set}: '{key}' existiert bereits"
                    )));
                }
            }
            list.push(new.clone());
            Ok(new)
        })
    }

    async fn patch_entity(
        &self,
        set: &str,
        key: &EntityKey,
        patch: &Value,
    ) -> Result<Value, StoreError> {
        self.write(set, |list| {
            let idx = list
                .iter()
                .position(|r| r.id() == key.id)
                .ok_or_else(|| not_found(set, key))?;
            let mut merged = value_of(&list[idx]);
            if let (Some(target), Some(p)) = (merged.as_object_mut(), patch.as_object()) {
                for (k, v) in p {
                    if k != "ID" && k != "IsActiveEntity" {
                        target.insert(k.clone(), v.clone());
                    }
                }
                // Der Titel eines Festwerts folgt seinem Code.
                if set == "ValueSet" {
                    if let Some(code) = p.get("value").cloned() {
                        target.insert("name".into(), code);
                    }
                }
            }
            let updated = DataRecord::from_json(&merged)?;
            list[idx] = updated.clone();
            Ok(value_of(&updated))
        })
    }

    async fn delete_entity(&self, set: &str, key: &EntityKey) -> Result<(), StoreError> {
        let mut inner = self.inner.lock().expect("ddic lock");
        let mut next = inner.sets.clone();
        let list = next.entry(set.to_string()).or_default();
        let idx = list
            .iter()
            .position(|r| r.id() == key.id)
            .ok_or_else(|| not_found(set, key))?;
        list.remove(idx);
        cascade_children(&mut next, set, &key.id.to_string());
        if set == "ValueListSet" {
            let lid = key.id.to_string();
            if let Some(values) = next.get_mut("ValueSet") {
                values.retain(|v| text(&value_of(v), "list_ID") != Some(lid.as_str()));
            }
            // Felder, die auf die Liste zeigen, verlieren die Zuordnung.
            if let Some(fields) = next.get_mut("FieldSet") {
                for f in fields.iter_mut() {
                    if text(&value_of(f), "value_list_ID") == Some(lid.as_str()) {
                        let mut v = value_of(f);
                        v["value_list_ID"] = Value::Null;
                        *f = DataRecord::from_json(&v)?;
                    }
                }
            }
        }
        if !self.transient && PERSISTED.contains(&set) {
            let json = build(&inner.base, &next)?;
            ddic_file::save(&self.path, &json)
                .map_err(|e| StoreError::Conflict(format!("cannot write ddic: {e:#}")))?;
            inner.base = json;
        }
        inner.sets = next;
        inner.revision += 1;
        Ok(())
    }

    async fn delete_entity_tree_only(&self, set: &str, key: &EntityKey) -> Result<(), StoreError> {
        // Ohne Entwuerfe gibt es nur aktive Saetze. Ein Draft-Schluessel meint
        // daher nichts Greifbares (z. B. Sibling-Aufraeumen nach dem Loeschen)
        // und darf den aktiven Satz mit derselben ID nicht treffen.
        if key.is_active_entity {
            self.delete_entity(set, key).await
        } else {
            Ok(())
        }
    }

    async fn read_sibling_entity(
        &self,
        set: &str,
        _key: &EntityKey,
    ) -> Result<DataRecord, StoreError> {
        Err(StoreError::Conflict(format!(
            "{set} does not support drafts"
        )))
    }

    async fn get_property(
        &self,
        set: &str,
        key: &EntityKey,
        property: &str,
    ) -> Result<Value, StoreError> {
        let record = self.read_entity(set, key, &ODataQuery::new()).await?;
        value_of(&record)
            .get(property)
            .cloned()
            .ok_or_else(|| StoreError::PropertyNotFound(property.to_string()))
    }

    async fn commit(&self) {}

    fn navigation(&mut self, single: bool) -> &mut Navigation {
        let _ = single;
        &mut self.navigation
    }

    fn set_navigations(&self, navigations: Vec<Navigation>) {
        *self.navigations.lock().expect("nav lock") = navigations;
    }
}

#[async_trait::async_trait]
impl DraftDataStore for DdicStore {
    async fn draft_edit(&self, set: &str, _key: &EntityKey) -> Result<Value, StoreError> {
        Err(StoreError::Conflict(format!(
            "{set} does not support drafts"
        )))
    }

    async fn draft_activate(&self, set: &str, _key: &EntityKey) -> Result<Value, StoreError> {
        Err(StoreError::Conflict(format!(
            "{set} does not support drafts"
        )))
    }

    async fn draft_prepare(&self, set: &str, _key: &EntityKey) -> Result<Value, StoreError> {
        Err(StoreError::Conflict(format!(
            "{set} does not support drafts"
        )))
    }
}
