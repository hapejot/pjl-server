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
use tracing::info;
use uuid::Uuid;

use crate::ddic_file;

/// Sets, die in `ddic.json` persistiert werden. Alle anderen Sets bleiben im Speicher.
const PERSISTED: [&str; 27] = [
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
    "EntitySeedSet",
    "EntitySeedValueSet",
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
        self.write_sets(set, |sets| f(sets.entry(set.to_string()).or_default()))
    }

    fn write_sets<T>(
        &self,
        set: &str,
        f: impl FnOnce(&mut Sets) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut inner = self.inner.lock().expect("ddic lock");
        let mut next = inner.sets.clone();
        let out = f(&mut next)?;
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
    for (name, values) in [
        ("FieldKindSet", &["Atom", "Measure"][..]),
        ("MeasureKindSet", &["Currency", "Unit"][..]),
        (
            "ValueHelpTypeSet",
            &["None", "FieldValueList", "EntityRef"][..],
        ),
        (
            "DisplayFormatSet",
            &["duration", "bytes", "bitrate", "date", "datetime"][..],
        ),
        (
            "EdmTypeSet",
            &[
                "Edm.Guid",
                "Edm.Boolean",
                "Edm.Int16",
                "Edm.Int32",
                "Edm.Byte",
                "Edm.SByte",
                "Edm.Int64",
                "Edm.Decimal",
                "Edm.Double",
                "Edm.Single",
                "Edm.Date",
                "Edm.DateTimeOffset",
                "Edm.DateTime",
                "Edm.Binary",
                "Edm.String",
            ][..],
        ),
        ("PrioritySet", &["high", "medium", "low"][..]),
        ("PlacementSet", &["Before", "After"][..]),
        ("ComponentKindSet", &["time_range"][..]),
        ("TimeUnitSet", &["minutes"][..]),
        (
            "SeedValueTypeSet",
            &["String", "Number", "Boolean", "Null", "Object", "Array"][..],
        ),
    ] {
        sets.insert(name.into(), FixedValueSet { name, values }.records());
    }
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
        EntitySeedSet::project(&mut sets, e, &entity_id);

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

struct BuildContext<'a> {
    base: &'a Value,
    records: HashMap<String, Vec<Value>>,
    package_names: HashMap<String, String>,
    entity_names: HashMap<String, String>,
    component_names: HashMap<String, String>,
}

impl<'a> BuildContext<'a> {
    fn new(base: &'a Value, sets: &'a Sets) -> Self {
        Self {
            base,
            records: sets
                .iter()
                .map(|(set, records)| (set.clone(), records.iter().map(value_of).collect()))
                .collect(),
            package_names: names_by_id(sets, "PackageSet"),
            entity_names: names_by_id(sets, "EntitySet"),
            component_names: names_by_id(sets, "ComponentSet"),
        }
    }

    fn records(&self, set: &str) -> &[Value] {
        self.records.get(set).map(Vec::as_slice).unwrap_or_default()
    }
}

fn required_name(value: &Value) -> Result<String, StoreError> {
    text(value, "name")
        .map(str::to_string)
        .ok_or_else(|| StoreError::Conflict("name fehlt".into()))
}

struct PackageSet<'a>(&'a BuildContext<'a>);

impl PackageSet<'_> {
    fn build(&self) -> Result<Vec<Value>, StoreError> {
        self.0
            .records("PackageSet")
            .iter()
            .map(|record| {
                let name = required_name(record)?;
                let mut obj = original(self.0.base, "packages", &name)
                    .and_then(|value| value.as_object().cloned())
                    .unwrap_or_default();
                obj.insert("name".into(), json!(name));
                Ok(Value::Object(obj))
            })
            .collect()
    }
}

struct EntitySet<'a>(&'a BuildContext<'a>);

impl EntitySet<'_> {
    fn prepare_create(record: &mut Map<String, Value>) {
        record.entry("draft_enabled").or_insert(json!(true));
    }

    fn on_create(sets: &mut Sets, entity: &DataRecord) -> Result<(), StoreError> {
        sets.entry("FieldSet".into())
            .or_default()
            .push(FieldSet::name_field(entity)?);
        Ok(())
    }

    fn build(&self) -> Result<Vec<Value>, StoreError> {
        self.0
            .records("EntitySet")
            .iter()
            .map(|record| self.build_entity(record))
            .collect()
    }

    fn build_entity(&self, record: &Value) -> Result<Value, StoreError> {
        let name = required_name(record)?;
        let ent_id = text(&record, "ID").unwrap_or_default().to_string();
        let mut obj = original(self.0.base, "entities", &name)
            .and_then(|o| o.as_object().cloned())
            .unwrap_or_default();
        let pkg = record
            .get("package_ID")
            .and_then(Value::as_str)
            .and_then(|id| self.0.package_names.get(id))
            .ok_or_else(|| {
                StoreError::Conflict(format!("Entitaet '{name}': Paket fehlt oder unbekannt"))
            })?;
        obj.insert("name".into(), json!(name));
        obj.insert("package".into(), json!(pkg));
        let plural = text(&record, "plural")
            .map(str::to_string)
            .unwrap_or_else(|| format!("{name}s"));
        obj.insert("type_name_plural".into(), json!(plural));
        match record.get("draft_enabled") {
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
            &record.get("type_name").cloned().unwrap_or(Value::Null),
        );
        match record
            .get("audited")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        {
            true => {
                obj.remove("audited");
            }
            false => {
                obj.insert("audited".into(), json!(false));
            }
        }
        set_array(&mut obj, "list_views", self.list_views(&ent_id));
        let filter_defaults = self.filter_defaults(&ent_id)?;
        if filter_defaults.is_empty() {
            obj.remove("filter_defaults");
        } else {
            obj.insert("filter_defaults".into(), Value::Object(filter_defaults));
        }
        let selection_fields = child_records(
            self.0.records("EntitySelectionFieldSet"),
            "entity_ID",
            &ent_id,
        )
        .into_iter()
        .filter_map(|field| text(field, "field").map(|field| json!(field)))
        .collect();
        set_array(&mut obj, "selection_fields", selection_fields);
        set_array(&mut obj, "facets", self.facets(&ent_id));
        let ui = self.ui(&ent_id);
        if ui.is_empty() {
            obj.remove("ui");
        } else {
            obj.insert("ui".into(), Value::Object(ui));
        }
        let orig_fields = obj
            .get("fields")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let fields = FieldSet(self.0).build_for(&ent_id, &orig_fields)?;
        obj.insert("fields".into(), Value::Array(fields));
        set_array(&mut obj, "seed", EntitySeedSet(self.0).build_for(&ent_id)?);
        Ok(Value::Object(obj))
    }

    fn list_views(&self, entity_id: &str) -> Vec<Value> {
        child_records(self.0.records("EntityListViewSet"), "entity_ID", entity_id)
            .into_iter()
            .map(|view| {
                let view_id = text(view, "ID").unwrap_or_default();
                let columns: Vec<Value> = child_records(
                    self.0.records("EntityListColumnSet"),
                    "list_view_ID",
                    view_id,
                )
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
            .collect()
    }

    fn filter_defaults(&self, entity_id: &str) -> Result<Map<String, Value>, StoreError> {
        let mut filter_defaults = Map::new();
        for filter in child_records(
            self.0.records("EntityFilterDefaultSet"),
            "entity_ID",
            entity_id,
        ) {
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
        Ok(filter_defaults)
    }

    fn facets(&self, entity_id: &str) -> Vec<Value> {
        child_records(self.0.records("EntityFacetSet"), "entity_ID", entity_id)
            .into_iter()
            .map(|facet| {
                let facet_id = text(facet, "ID").unwrap_or_default();
                let fields: Vec<Value> =
                    child_records(self.0.records("EntityFacetFieldSet"), "facet_ID", facet_id)
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
            .collect()
    }

    fn ui(&self, entity_id: &str) -> Map<String, Value> {
        let hidden_fields: Vec<Value> = child_records(
            self.0.records("EntityHiddenFieldSet"),
            "entity_ID",
            entity_id,
        )
        .into_iter()
        .filter_map(|field| text(field, "field").map(|field| json!(field)))
        .collect();
        let hide_facets: Vec<Value> = child_records(
            self.0.records("EntityHiddenFacetSet"),
            "entity_ID",
            entity_id,
        )
        .into_iter()
        .filter_map(|facet| text(facet, "facet").map(|facet| json!(facet)))
        .collect();
        let sections: Vec<Value> =
            child_records(self.0.records("EntityUiSectionSet"), "entity_ID", entity_id)
                .into_iter()
                .map(|section| {
                    json!({
                        "key": text(section, "key"), "title": text(section, "title"),
                        "fragment": text(section, "fragment"), "anchor": text(section, "anchor"),
                        "placement": text(section, "placement"),
                    })
                })
                .collect();
        let list_columns: Vec<Value> = child_records(
            self.0.records("EntityUiListColumnSet"),
            "entity_ID",
            entity_id,
        )
        .into_iter()
        .map(|column| {
            json!({
                "key": text(column, "key"), "header": text(column, "header"),
                "fragment": text(column, "fragment"), "anchor": text(column, "anchor"),
                "placement": text(column, "placement"), "width": text(column, "width"),
            })
        })
        .collect();
        let list_templates: Vec<Value> = child_records(
            self.0.records("EntityUiListTemplateSet"),
            "entity_ID",
            entity_id,
        )
        .into_iter()
        .map(|template| {
            json!({
                "key": text(template, "key"), "label": text(template, "label"),
                "fragment": text(template, "fragment"),
            })
        })
        .collect();
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
        ui
    }
}

struct EntitySeedSet<'a>(&'a BuildContext<'a>);

impl EntitySeedSet<'_> {
    fn prepare_write(record: &mut Map<String, Value>) -> Result<(), StoreError> {
        let Some(value) = record.get("order") else {
            return Ok(());
        };
        if value.is_null() || value.as_str().is_some_and(|value| value.trim().is_empty()) {
            record.remove("order");
            return Ok(());
        }
        let order = match value {
            Value::String(value) => value.trim().parse::<u32>().ok(),
            value => value.as_u64().and_then(|value| u32::try_from(value).ok()),
        }
        .ok_or_else(|| {
            StoreError::Conflict("Seed-Position muss eine nichtnegative ganze Zahl sein".into())
        })?;
        record.insert("order".into(), json!(order));
        record.insert(
            "name".into(),
            json!(format!("Seed {}", u64::from(order) + 1)),
        );
        Ok(())
    }

    fn prepare_create(sets: &Sets, record: &DataRecord) -> Result<DataRecord, StoreError> {
        let mut value = value_of(record);
        if value.get("order").is_none_or(Value::is_null) {
            let order = sets
                .get("EntitySeedSet")
                .into_iter()
                .flatten()
                .filter(|row| row.get("entity_ID") == record.get("entity_ID"))
                .filter_map(|row| row.get("order").and_then(|value| value.as_u64()))
                .max()
                .map_or(Some(0), |order| order.checked_add(1))
                .ok_or_else(|| StoreError::Conflict("Seed-Position ist zu gross".into()))?;
            value["order"] = json!(order);
        }
        Self::prepare_write(
            value
                .as_object_mut()
                .ok_or_else(|| StoreError::Serialize("Objekt erwartet".into()))?,
        )?;
        DataRecord::from_json(&value)
    }

    fn project(sets: &mut Sets, entity: &Value, entity_id: &str) {
        for (order, row) in items(entity, "seed").iter().enumerate() {
            let identity = format!("{}/{order}", text(entity, "name").unwrap_or_default());
            let seed_id = id_for("EntitySeedSet", &identity);
            push_record(
                sets,
                "EntitySeedSet",
                seed_id.clone(),
                json!({
                    "name": format!("Seed {}", order + 1), "order": order, "entity_ID": entity_id,
                }),
            );
            if let Some(fields) = row.as_object() {
                for (field, value) in fields {
                    let value_type = match value {
                        Value::String(_) => "String",
                        Value::Number(_) => "Number",
                        Value::Bool(_) => "Boolean",
                        Value::Null => "Null",
                        Value::Object(_) => "Object",
                        Value::Array(_) => "Array",
                    };
                    let value = match value {
                        Value::String(value) => value.clone(),
                        value => value.to_string(),
                    };
                    push_record(
                        sets,
                        "EntitySeedValueSet",
                        id_for("EntitySeedValueSet", &format!("{identity}/{field}")),
                        json!({"seed_ID": seed_id, "field": field, "value": value, "value_type": value_type}),
                    );
                }
            }
        }
    }

    fn build_for(&self, entity_id: &str) -> Result<Vec<Value>, StoreError> {
        child_records(self.0.records("EntitySeedSet"), "entity_ID", entity_id)
            .into_iter()
            .map(|row| {
                let seed_id = text(row, "ID").unwrap_or_default();
                let mut fields = Map::new();
                for record in
                    child_records(self.0.records("EntitySeedValueSet"), "seed_ID", seed_id)
                {
                    let field = text(record, "field")
                        .ok_or_else(|| StoreError::Conflict("Seed-Feldname fehlt".into()))?;
                    let value = EntitySeedValueSet::value(record)?;
                    if fields.insert(field.into(), value).is_some() {
                        return Err(StoreError::Conflict(format!(
                            "Seed-Feld '{field}' ist doppelt"
                        )));
                    }
                }
                Ok(Value::Object(fields))
            })
            .collect()
    }
}

struct EntitySeedValueSet;

impl EntitySeedValueSet {
    fn value(record: &Value) -> Result<Value, StoreError> {
        let raw = record
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let value_type = text(record, "value_type").unwrap_or("String");
        if value_type == "String" {
            return Ok(json!(raw));
        }
        if value_type == "Null" {
            return Ok(Value::Null);
        }
        let parsed = serde_json::from_str::<Value>(raw).ok();
        match (value_type, parsed) {
            ("Number", Some(value @ Value::Number(_)))
            | ("Boolean", Some(value @ Value::Bool(_)))
            | ("Object", Some(value @ Value::Object(_)))
            | ("Array", Some(value @ Value::Array(_))) => Ok(value),
            _ => Err(StoreError::Conflict(format!(
                "Seed-Feld '{}': Wert muss vom Typ {value_type} sein",
                text(record, "field").unwrap_or_default()
            ))),
        }
    }
}

struct FixedValueSet<'a> {
    name: &'a str,
    values: &'a [&'a str],
}

impl FixedValueSet<'_> {
    fn records(&self) -> Vec<DataRecord> {
        self.values
            .iter()
            .map(|value| {
                rec(
                    self.name,
                    id_for(self.name, value),
                    json!({ "name": value, "value": value }),
                )
            })
            .collect()
    }
}

struct FieldSet<'a>(&'a BuildContext<'a>);

impl FieldSet<'_> {
    fn prepare_write(record: &mut Map<String, Value>) -> Result<(), StoreError> {
        for key in ["max_length", "precision", "scale"] {
            let Some(value) = record.get(key) else {
                continue;
            };
            let number = match value {
                Value::Null => continue,
                Value::String(text) if text.trim().is_empty() => {
                    record.insert(key.into(), Value::Null);
                    continue;
                }
                Value::String(text) => text.trim().parse::<u32>().ok(),
                value => value.as_u64().and_then(|number| u32::try_from(number).ok()),
            }
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "{key} muss eine ganze Zahl zwischen 0 und {} sein",
                    u32::MAX
                ))
            })?;
            record.insert(key.into(), json!(number));
        }
        Ok(())
    }

    fn name_field(entity: &DataRecord) -> Result<DataRecord, StoreError> {
        let name = text(&value_of(entity), "name")
            .map(str::to_string)
            .unwrap_or_else(|| entity.id().to_string());
        DataRecord::from_json(&json!({
            "ID": id_for("FieldSet", &format!("{name}.Name")),
            "IsActiveEntity": true,
            "entity_ID": entity.id().to_string(),
            "name": "Name",
            "label": "Name",
            "kind": "Atom",
            "type": "Edm.String",
        }))
    }

    fn build_for(&self, ent_id: &str, orig_fields: &[Value]) -> Result<Vec<Value>, StoreError> {
        let field_recs = self.0.records("FieldSet");
        let mut fields: Vec<Value> = orig_fields
            .iter()
            .filter(|f| f.get("Atom").is_none() && f.get("Measure").is_none())
            .cloned()
            .collect();
        for fr in field_recs
            .iter()
            .filter(|f| text(f, "entity_ID") == Some(ent_id))
        {
            fields.push(self.build_field(fr, orig_fields)?);
        }
        Ok(fields)
    }

    fn build_field(&self, fr: &Value, orig_fields: &[Value]) -> Result<Value, StoreError> {
        let fname = required_name(fr)?;
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
            return Ok(json!({ "Measure": spec }));
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
        let side_effects: Vec<Value> =
            child_records(self.0.records("FieldSideEffectSet"), "field_ID", field_id)
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
        match self.value_help(fr)? {
            Some(value_help) => {
                spec.insert("value_list".into(), value_help);
            }
            None => {
                spec.remove("value_list");
            }
        }
        Ok(json!({ "Atom": spec }))
    }

    fn value_help(&self, fr: &Value) -> Result<Option<Value>, StoreError> {
        let prefer_dialog = fr
            .get("prefer_dialog")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        match text(&fr, "value_help_type").unwrap_or("None") {
            "None" => Ok(None),
            "FieldValueList" => match text(&fr, "value_list_ID") {
                Some(list_id) => Ok(Some(json!({ "FieldValueList": {
                    "list_id": list_id,
                    "prefer_dialog": prefer_dialog,
                }}))),
                None => Ok(None),
            },
            "EntityRef" => {
                let field_id = text(fr, "ID").unwrap_or_default();
                let filters: Vec<Value> =
                    child_records(self.0.records("ValueHelpFilterSet"), "field_ID", field_id)
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
                Ok(Some(json!({ "EntityRef": config })))
            }
            other => Err(StoreError::Conflict(format!(
                "Feld '{}': unbekannter Wertehilfetyp '{other}'",
                required_name(fr)?
            ))),
        }
    }
}

struct RelationSet<'a>(&'a BuildContext<'a>);

impl RelationSet<'_> {
    fn build(&self) -> Result<Vec<Value>, StoreError> {
        let base = self.0.base;
        let ent_names = &self.0.entity_names;
        let mut relationships = Vec::new();
        for r in self.0.records("RelationSet") {
            let name = required_name(&r)?;
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
        Ok(relationships)
    }
}

struct AppSet<'a>(&'a BuildContext<'a>);

impl AppSet<'_> {
    fn build(&self) -> Result<Vec<Value>, StoreError> {
        let base = self.0.base;
        let pkg_names = &self.0.package_names;
        let ent_names = &self.0.entity_names;
        let mut apps = Vec::new();
        let app_module_recs = self.0.records("AppModuleSet");
        let app_extension_recs = self.0.records("AppControllerExtensionSet");
        for r in self.0.records("AppSet") {
            let name = required_name(&r)?;
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
        Ok(apps)
    }
}

struct ComponentSet<'a>(&'a BuildContext<'a>);

impl ComponentSet<'_> {
    fn build(&self) -> Result<Vec<Value>, StoreError> {
        let base = self.0.base;
        let mut components = Vec::new();
        for r in self.0.records("ComponentSet") {
            let name = required_name(&r)?;
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
                &text(r, "unit")
                    .map(|unit| json!(unit))
                    .unwrap_or(Value::Null),
            );
            put(
                &mut obj,
                "step_minutes",
                &r.get("step_minutes").cloned().unwrap_or(Value::Null),
            );
            components.push(Value::Object(obj));
        }
        Ok(components)
    }
}

struct ComponentUseSet<'a>(&'a BuildContext<'a>);

impl ComponentUseSet<'_> {
    fn build(&self) -> Result<Vec<Value>, StoreError> {
        let base = self.0.base;
        let ent_names = &self.0.entity_names;
        let component_names = &self.0.component_names;
        let mut component_uses = Vec::new();
        let component_role_recs = self.0.records("ComponentUseRoleSet");
        for r in self.0.records("ComponentUseSet") {
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
        Ok(component_uses)
    }
}

struct ValueListSet<'a>(&'a BuildContext<'a>);

impl ValueListSet<'_> {
    fn build(&self) -> Result<Vec<Value>, StoreError> {
        let mut value_lists = Vec::new();
        let value_recs = self.0.records("ValueSet");
        for r in self.0.records("ValueListSet") {
            let name = required_name(&r)?;
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
        Ok(value_lists)
    }
}

fn build(base: &Value, sets: &Sets) -> Result<Value, StoreError> {
    let context = BuildContext::new(base, sets);
    let mut out = base.as_object().cloned().unwrap_or_default();
    out.insert(
        "packages".into(),
        Value::Array(PackageSet(&context).build()?),
    );
    out.insert(
        "entities".into(),
        Value::Array(EntitySet(&context).build()?),
    );
    out.insert(
        "relationships".into(),
        Value::Array(RelationSet(&context).build()?),
    );
    out.insert("apps".into(), Value::Array(AppSet(&context).build()?));
    set_array(&mut out, "components", ComponentSet(&context).build()?);
    set_array(
        &mut out,
        "component_uses",
        ComponentUseSet(&context).build()?,
    );
    set_array(&mut out, "value_lists", ValueListSet(&context).build()?);
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
    if set == "EntitySeedSet" {
        return None;
    }
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
        "EntitySeedValueSet" => "seed_ID",
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
            "EntityListColumnSet" | "EntityFacetFieldSet" | "EntitySeedValueSet" => {
                text(v, "field")?
            }
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
    const CASCADES: [(&str, &str, &str); 22] = [
        ("EntitySet", "FieldSet", "entity_ID"),
        ("EntitySet", "EntitySeedSet", "entity_ID"),
        ("EntitySeedSet", "EntitySeedValueSet", "seed_ID"),
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
        info!("designer create entity: {}", set);
        let mut v = data.clone();
        let obj = v
            .as_object_mut()
            .ok_or_else(|| StoreError::Serialize("Objekt erwartet".into()))?;
        if set == "EntitySet" {
            EntitySet::prepare_create(obj);
        }
        if set == "FieldSet" {
            FieldSet::prepare_write(obj)?;
        }
        if set == "EntitySeedSet" {
            obj.entry("name").or_insert(json!("Seed"));
            EntitySeedSet::prepare_write(obj)?;
        }
        if set == "EntitySeedValueSet" {
            obj.entry("value_type").or_insert(json!("String"));
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
        self.write_sets(set, |sets| {
            let new = if set == "EntitySeedSet" {
                EntitySeedSet::prepare_create(sets, &new)?
            } else {
                new
            };
            let list = sets.entry(set.to_string()).or_default();
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
            if set == "EntitySet" {
                EntitySet::on_create(sets, &new)?;
            }
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
                if set == "FieldSet" {
                    FieldSet::prepare_write(target)?;
                }
                if set == "EntitySeedSet" {
                    EntitySeedSet::prepare_write(target)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn store_with_package() -> (TempDir, DdicStore, DataRecord) {
        let directory = TempDir::new().unwrap();
        let store = DdicStore::open(directory.path().join("ddic.json")).unwrap();
        let package = store
            .create_entity("PackageSet", &json!({"name": "test"}))
            .await
            .unwrap();
        (directory, store, package)
    }

    #[test]
    fn entity_seeds_are_projected_and_preserve_json_types() {
        let base = json!({
            "packages": [{"name": "test"}],
            "entities": [{
                "name": "Currency", "package": "test", "type_name_plural": "Currencies",
                "fields": [{"Atom": {"name": "code", "label": "Code", "edm_type": "Edm.String"}}],
                "seed": [{"code": "EUR", "name": "Euro", "decimals": 2, "active": true,
                    "optional": null, "aliases": ["euro"], "details": {"symbol": "EUR"}}]
            }],
            "relationships": [], "apps": []
        });
        let sets = project(&base);
        let rows = sets.get("EntitySeedSet").expect("seed rows in designer");
        assert_eq!(rows.len(), 1);
        let values = sets
            .get("EntitySeedValueSet")
            .expect("seed values in designer");
        assert_eq!(values.len(), 7);
        let decimals = values
            .iter()
            .find(|value| value.get("field") == Some(json!("decimals")))
            .unwrap();
        assert_eq!(decimals.get("value"), Some(json!("2")));
        assert_eq!(decimals.get("value_type"), Some(json!("Number")));
        let rebuilt = build(&base, &sets).unwrap();
        assert_eq!(rebuilt["entities"][0]["seed"], base["entities"][0]["seed"]);
    }

    #[tokio::test]
    async fn seed_sessions_preserve_row_order_and_entity_deletion_cascades() {
        let (_directory, store, package) = store_with_package().await;
        let entity = store
            .create_entity(
                "EntitySet",
                &json!({
                    "name": "Currency", "package_ID": package.id().to_string()
                }),
            )
            .await
            .unwrap();
        let baseline = store.snapshot();
        let working = store.fork();
        let mut rows = Vec::new();
        for code in ["EUR", "USD"] {
            let row = working
                .create_entity(
                    "EntitySeedSet",
                    &json!({
                        "entity_ID": entity.id().to_string()
                    }),
                )
                .await
                .unwrap();
            working
                .create_entity(
                    "EntitySeedValueSet",
                    &json!({
                        "seed_ID": row.id().to_string(), "field": "code", "value": code
                    }),
                )
                .await
                .unwrap();
            rows.push(row);
        }
        assert_eq!(rows[0].get("order"), Some(json!(0)));
        assert_eq!(rows[1].get("order"), Some(json!(1)));
        working
            .patch_entity(
                "EntitySeedSet",
                &EntityKey::new_active(rows[0].id().to_string()),
                &json!({"order": "5"}),
            )
            .await
            .unwrap();
        assert_eq!(store.count("EntitySeedSet", &ODataQuery::new()).await, 0);
        store.save_session(&baseline, &working).unwrap();
        let disk: Value = serde_json::from_slice(&std::fs::read(&store.path).unwrap()).unwrap();
        assert_eq!(
            disk["entities"][0]["seed"],
            json!([{"code": "USD"}, {"code": "EUR"}])
        );
        store
            .delete_entity("EntitySet", &EntityKey::new_active(entity.id().to_string()))
            .await
            .unwrap();
        assert_eq!(store.count("EntitySeedSet", &ODataQuery::new()).await, 0);
        assert_eq!(
            store.count("EntitySeedValueSet", &ODataQuery::new()).await,
            0
        );
    }

    #[tokio::test]
    async fn entity_seeds_can_be_edited_through_designer_navigation() {
        use axum::{
            body::Body,
            http::{Method, Request, StatusCode},
        };
        use tower::ServiceExt;

        async fn request(
            state: &crate::AppState,
            method: Method,
            uri: &str,
            body: Value,
            expected: StatusCode,
        ) -> Value {
            let response = crate::srv::router()
                .with_state(state.clone())
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header("content-type", "application/json")
                        .body(Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            assert_eq!(
                status,
                expected,
                "{uri}: {}",
                String::from_utf8_lossy(&bytes)
            );
            if bytes.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&bytes).unwrap()
            }
        }

        let (_directory, store, package) = store_with_package().await;
        let entity = store
            .create_entity(
                "EntitySet",
                &json!({
                    "name": "Currency", "package_ID": package.id().to_string()
                }),
            )
            .await
            .unwrap();
        let path = store.path.clone();
        let store = std::sync::Arc::new(store);
        let state = crate::AppState::designer(store.clone());
        let metadata = crate::srv::router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/$metadata")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(metadata.status(), StatusCode::OK);
        let metadata = axum::body::to_bytes(metadata.into_body(), usize::MAX)
            .await
            .unwrap();
        let metadata = String::from_utf8(metadata.to_vec()).unwrap();
        assert!(metadata.contains("_Seeds/@UI.LineItem"));
        assert!(metadata.contains("_Values/@UI.LineItem"));
        let seeds_uri = format!("/EntitySet({})/_Seeds", entity.id());
        let seed = request(
            &state,
            Method::POST,
            &seeds_uri,
            json!({}),
            StatusCode::CREATED,
        )
        .await;
        let seed_uri = format!("/EntitySeedSet({})", seed["ID"].as_str().unwrap());
        let values_uri = format!("{seed_uri}/_Values");
        let code = request(
            &state,
            Method::POST,
            &values_uri,
            json!({
                "field": "code", "value": "EUR", "value_type": "String"
            }),
            StatusCode::CREATED,
        )
        .await;
        let decimals = request(
            &state,
            Method::POST,
            &values_uri,
            json!({
                "field": "decimal_places", "value": "2", "value_type": "Number"
            }),
            StatusCode::CREATED,
        )
        .await;
        request(
            &state,
            Method::POST,
            &values_uri,
            json!({
                "field": "name", "value": "Euro"
            }),
            StatusCode::CREATED,
        )
        .await;
        let values = request(
            &state,
            Method::GET,
            &values_uri,
            Value::Null,
            StatusCode::OK,
        )
        .await;
        assert_eq!(values["value"].as_array().unwrap().len(), 3);
        let disk: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            disk["entities"][0]["seed"],
            json!([{"code": "EUR", "decimal_places": 2, "name": "Euro"}])
        );
        let decimals_uri = format!("/EntitySeedValueSet({})", decimals["ID"].as_str().unwrap());
        request(
            &state,
            Method::PATCH,
            &decimals_uri,
            json!({"value": "0"}),
            StatusCode::NO_CONTENT,
        )
        .await;
        let before = std::fs::read(&path).unwrap();
        request(
            &state,
            Method::PATCH,
            &decimals_uri,
            json!({"value": "invalid"}),
            StatusCode::CONFLICT,
        )
        .await;
        assert_eq!(std::fs::read(&path).unwrap(), before);
        request(
            &state,
            Method::POST,
            &values_uri,
            json!({"field": "code", "value": "USD"}),
            StatusCode::CONFLICT,
        )
        .await;
        let reopened = DdicStore::open(&path).unwrap();
        assert_eq!(reopened.count("EntitySeedSet", &ODataQuery::new()).await, 1);
        let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["entities"][0]["seed"][0]["decimal_places"], 0);
        let code_uri = format!("/EntitySeedValueSet({})", code["ID"].as_str().unwrap());
        request(
            &state,
            Method::DELETE,
            &code_uri,
            Value::Null,
            StatusCode::NO_CONTENT,
        )
        .await;
        let disk: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(disk["entities"][0]["seed"][0].get("code").is_none());
        request(
            &state,
            Method::DELETE,
            &seed_uri,
            Value::Null,
            StatusCode::NO_CONTENT,
        )
        .await;
        assert_eq!(
            store.count("EntitySeedValueSet", &ODataQuery::new()).await,
            0
        );
        assert_eq!(
            DdicStore::open(&path)
                .unwrap()
                .count("EntitySeedSet", &ODataQuery::new())
                .await,
            0
        );
        let disk: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(disk["entities"][0].get("seed").is_none());
    }

    #[tokio::test]
    async fn designer_fixed_value_fields_have_dropdowns() {
        use axum::{
            body::Body,
            http::{Request, StatusCode},
        };
        use tower::ServiceExt;

        let directory = TempDir::new().unwrap();
        let path = directory.path().join("ddic.json");
        let state = crate::AppState::designer(std::sync::Arc::new(DdicStore::open(&path).unwrap()));
        for (target, set, expected) in [
            ("Field/kind", "FieldKindSet", &["Atom", "Measure"][..]),
            (
                "Field/measure_kind",
                "MeasureKindSet",
                &["Currency", "Unit"][..],
            ),
            (
                "Field/value_help_type",
                "ValueHelpTypeSet",
                &["None", "FieldValueList", "EntityRef"][..],
            ),
            (
                "Field/display_format",
                "DisplayFormatSet",
                &["duration", "bytes", "bitrate", "date", "datetime"][..],
            ),
            (
                "Field/type",
                "EdmTypeSet",
                &[
                    "Edm.Guid",
                    "Edm.Boolean",
                    "Edm.Int16",
                    "Edm.Int32",
                    "Edm.Byte",
                    "Edm.SByte",
                    "Edm.Int64",
                    "Edm.Decimal",
                    "Edm.Double",
                    "Edm.Single",
                    "Edm.Date",
                    "Edm.DateTimeOffset",
                    "Edm.DateTime",
                    "Edm.Binary",
                    "Edm.String",
                ][..],
            ),
            (
                "EntityListColumn/priority",
                "PrioritySet",
                &["high", "medium", "low"][..],
            ),
            (
                "EntityUiSection/placement",
                "PlacementSet",
                &["Before", "After"][..],
            ),
            (
                "EntityUiListColumn/placement",
                "PlacementSet",
                &["Before", "After"][..],
            ),
            ("Component/kind", "ComponentKindSet", &["time_range"][..]),
            ("Component/unit", "TimeUnitSet", &["minutes"][..]),
            (
                "EntitySeedValue/value_type",
                "SeedValueTypeSet",
                &["String", "Number", "Boolean", "Null", "Object", "Array"][..],
            ),
        ] {
            let options_uri = format!("/{set}?$orderby=value");
            for uri in ["/$metadata", options_uri.as_str()] {
                let response = crate::srv::router()
                    .with_state(state.clone())
                    .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK, "{uri}");
                let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap();
                let body = String::from_utf8(bytes.to_vec()).unwrap();
                if uri == "/$metadata" {
                    let target = format!("Target=\"Service.{target}\"");
                    let annotations = body
                        .split(target.as_str())
                        .nth(1)
                        .unwrap()
                        .split("</Annotations>")
                        .next()
                        .unwrap();
                    assert!(
                        annotations.contains("Common.ValueListWithFixedValues"),
                        "{annotations}"
                    );
                    assert!(annotations.contains(set), "{annotations}");
                    assert!(
                        annotations.contains("ValueListParameterOut"),
                        "{annotations}"
                    );
                    assert!(
                        annotations.contains("Property=\"ValueListProperty\" String=\"value\""),
                        "{annotations}"
                    );
                } else {
                    let options: Value = serde_json::from_str(&body).unwrap();
                    let values = options["value"].as_array().unwrap();
                    let mut expected = expected.to_vec();
                    expected.sort_unstable();
                    assert_eq!(values.len(), expected.len(), "{set}");
                    for (value, expected) in values.iter().zip(expected) {
                        assert_eq!(value["value"], expected, "{set}");
                        assert_eq!(value["name"], expected, "{set}");
                    }
                    assert!(
                        values
                            .iter()
                            .all(|value| value.get("IsActiveEntity").is_none())
                    );
                }
            }
        }
        let disk: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(disk["entities"], json!([]));
    }

    #[tokio::test]
    async fn optional_dropdowns_can_be_cleared() {
        let (_directory, store, package) = store_with_package().await;
        let entity = store
            .create_entity(
                "EntitySet",
                &json!({
                    "name": "Catalog", "package_ID": package.id().to_string()
                }),
            )
            .await
            .unwrap();
        let baseline = store.snapshot();
        let working = store.fork();
        let view = working
            .create_entity(
                "EntityListViewSet",
                &json!({
                    "entity_ID": entity.id().to_string(), "name": "default"
                }),
            )
            .await
            .unwrap();
        working
            .create_entity(
                "EntityListColumnSet",
                &json!({
                    "list_view_ID": view.id().to_string(), "field": "Name"
                }),
            )
            .await
            .unwrap();
        store.save_session(&baseline, &working).unwrap();
        for (set, property, data, pointer, expected) in [
            (
                "FieldSet",
                "display_format",
                json!({
                    "entity_ID": entity.id().to_string(), "name": "Formatted",
                    "type": "Edm.Int64", "display_format": "bytes"
                }),
                "/entities/0/fields/1/Atom/ui",
                Value::Null,
            ),
            (
                "EntityListColumnSet",
                "priority",
                json!({
                    "list_view_ID": view.id().to_string(), "field": "Formatted", "priority": "high"
                }),
                "/entities/0/list_views/0/columns/1/priority",
                Value::Null,
            ),
            (
                "EntityUiSectionSet",
                "placement",
                json!({
                    "entity_ID": entity.id().to_string(), "key": "Details", "title": "Details",
                    "fragment": "Details", "anchor": "GeneralSection", "placement": "Before"
                }),
                "/entities/0/ui/sections/0/placement",
                Value::Null,
            ),
            (
                "EntityUiListColumnSet",
                "placement",
                json!({
                    "entity_ID": entity.id().to_string(), "key": "Extra", "header": "Extra",
                    "fragment": "Extra", "anchor": "Name", "placement": "Before"
                }),
                "/entities/0/ui/list_columns/0/placement",
                Value::Null,
            ),
            (
                "ComponentSet",
                "unit",
                json!({
                    "name": "Range", "kind": "time_range", "unit": "minutes"
                }),
                "/components/0/unit",
                Value::Null,
            ),
        ] {
            let record = store.create_entity(set, &data).await.unwrap();
            let key = EntityKey::new_active(record.id().to_string());
            for value in [json!(""), Value::Null] {
                let mut patch = Map::new();
                patch.insert(property.into(), value);
                store
                    .patch_entity(set, &key, &Value::Object(patch))
                    .await
                    .unwrap();
                let disk: Value =
                    serde_json::from_slice(&std::fs::read(&store.path).unwrap()).unwrap();
                assert_eq!(
                    disk.pointer(pointer).unwrap_or(&Value::Null),
                    &expected,
                    "{set}/{property}"
                );
                DdicStore::open(&store.path).unwrap();
            }
        }
    }

    #[tokio::test]
    async fn numeric_field_properties_accept_strings_on_create_and_patch() {
        let (_directory, store, package) = store_with_package().await;
        let entity = store
            .create_entity(
                "EntitySet",
                &json!({
                    "name": "Catalog", "package_ID": package.id().to_string()
                }),
            )
            .await
            .unwrap();
        let field = store
            .create_entity(
                "FieldSet",
                &json!({
                    "name": "Code", "entity_ID": entity.id().to_string(),
                    "type": "Edm.String", "max_length": "100", "precision": "18", "scale": "2"
                }),
            )
            .await
            .unwrap();
        assert_eq!(field.get("max_length"), Some(json!(100)));
        assert_eq!(field.get("precision"), Some(json!(18)));
        assert_eq!(field.get("scale"), Some(json!(2)));
        let key = EntityKey::new_active(field.id().to_string());
        let updated = store
            .patch_entity(
                "FieldSet",
                &key,
                &json!({
                    "max_length": "200", "precision": "", "scale": null
                }),
            )
            .await
            .unwrap();
        assert_eq!(updated["max_length"], 200);
        let disk: Value = serde_json::from_slice(&std::fs::read(&store.path).unwrap()).unwrap();
        let atom = &disk["entities"][0]["fields"][1]["Atom"];
        assert_eq!(atom["max_length"], 200);
        assert!(atom.get("precision").is_none());
        assert!(atom.get("scale").is_none());
        let reopened = DdicStore::open(&store.path).unwrap();
        let fields = reopened
            .get_collection("FieldSet", &ODataQuery::new())
            .await
            .unwrap();
        let saved = fields
            .iter()
            .find(|field| field.get("name") == Some(json!("Code")))
            .unwrap();
        assert_eq!(saved.get("max_length"), Some(json!(200)));
    }

    #[test]
    fn numeric_field_properties_validate_u32_values() {
        for key in ["max_length", "precision", "scale"] {
            for (input, expected) in [
                (json!(100), json!(100)),
                (json!(" 100 "), json!(100)),
                (json!(0), json!(0)),
                (json!(u32::MAX), json!(u32::MAX)),
                (json!(u32::MAX.to_string()), json!(u32::MAX)),
                (Value::Null, Value::Null),
                (json!(" "), Value::Null),
            ] {
                let mut record = Map::new();
                record.insert(key.into(), input.clone());
                FieldSet::prepare_write(&mut record).unwrap();
                assert_eq!(record[key], expected, "{key}: {input}");
            }
            for input in [
                json!(-1),
                json!("-1"),
                json!(1.5),
                json!("1.5"),
                json!(u64::from(u32::MAX) + 1),
                json!("4294967296"),
                json!(true),
                json!("abc"),
            ] {
                let mut record = Map::new();
                record.insert(key.into(), input.clone());
                let error = FieldSet::prepare_write(&mut record).unwrap_err();
                assert!(error.to_string().contains(key), "{key}: {input}: {error}");
            }
        }
    }

    #[tokio::test]
    async fn entity_creation_rolls_back_fields_on_validation_error_or_duplicate() {
        let (_directory, store, package) = store_with_package().await;
        let before = std::fs::read(&store.path).unwrap();
        let revision = store.snapshot().revision;
        assert!(
            store
                .create_entity("EntitySet", &json!({"name": "Invalid"}))
                .await
                .is_err()
        );
        assert_eq!(store.count("EntitySet", &ODataQuery::new()).await, 0);
        assert_eq!(store.count("FieldSet", &ODataQuery::new()).await, 0);
        assert_eq!(std::fs::read(&store.path).unwrap(), before);
        assert_eq!(store.snapshot().revision, revision);

        let entity = json!({"name": "Catalog", "package_ID": package.id().to_string()});
        store.create_entity("EntitySet", &entity).await.unwrap();
        let before = std::fs::read(&store.path).unwrap();
        let revision = store.snapshot().revision;
        assert!(store.create_entity("EntitySet", &entity).await.is_err());
        assert_eq!(store.count("EntitySet", &ODataQuery::new()).await, 1);
        assert_eq!(store.count("FieldSet", &ODataQuery::new()).await, 1);
        assert_eq!(std::fs::read(&store.path).unwrap(), before);
        assert_eq!(store.snapshot().revision, revision);
    }

    #[tokio::test]
    async fn entity_creation_in_session_saves_name_field_with_entity() {
        let (_directory, store, package) = store_with_package().await;
        let baseline = store.snapshot();
        let working = store.fork();
        let before = std::fs::read(&store.path).unwrap();
        let entity = working
            .create_entity(
                "EntitySet",
                &json!({
                    "name": "Catalog", "package_ID": package.id().to_string()
                }),
            )
            .await
            .unwrap();
        let fields = working
            .get_collection("FieldSet", &ODataQuery::new())
            .await
            .unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].get("name"), Some(json!("Name")));
        assert_eq!(
            fields[0].get("entity_ID"),
            Some(json!(entity.id().to_string()))
        );
        assert_eq!(store.count("FieldSet", &ODataQuery::new()).await, 0);
        assert_eq!(std::fs::read(&store.path).unwrap(), before);

        store.save_session(&baseline, &working).unwrap();
        let reopened = DdicStore::open(&store.path).unwrap();
        let saved = reopened
            .get_collection("FieldSet", &ODataQuery::new())
            .await
            .unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].id(), fields[0].id());
        assert_eq!(saved[0].get("name"), Some(json!("Name")));
    }

    #[tokio::test]
    async fn saving_existing_entity_does_not_recreate_deleted_name_field() {
        let (_directory, store, package) = store_with_package().await;
        store
            .create_entity(
                "EntitySet",
                &json!({
                    "name": "Catalog", "package_ID": package.id().to_string()
                }),
            )
            .await
            .unwrap();
        let fields = store
            .get_collection("FieldSet", &ODataQuery::new())
            .await
            .unwrap();
        store
            .delete_entity(
                "FieldSet",
                &EntityKey::new_active(fields[0].id().to_string()),
            )
            .await
            .unwrap();
        let reopened = DdicStore::open(&store.path).unwrap();
        let entities = reopened
            .get_collection("EntitySet", &ODataQuery::new())
            .await
            .unwrap();
        reopened
            .patch_entity(
                "EntitySet",
                &EntityKey::new_active(entities[0].id().to_string()),
                &json!({"plural": "Catalog entries"}),
            )
            .await
            .unwrap();
        assert_eq!(reopened.count("FieldSet", &ODataQuery::new()).await, 0);
        assert_eq!(
            DdicStore::open(&store.path)
                .unwrap()
                .count("FieldSet", &ODataQuery::new())
                .await,
            0
        );
    }

    #[test]
    fn build_preserves_entity_configuration_and_unedited_properties() {
        let base = json!({
            "packages": [{"name": "test"}],
            "entities": [{
                "name": "Catalog", "package": "test", "type_name_plural": "Catalogs",
                "draft_enabled": false, "primary_key": "Name",
                "fields": [{"Atom": {"name": "Name", "label": "Name", "edm_type": "Edm.String", "max_length": 80, "link": true, "side_effects": ["Name"]}}],
                "list_views": [{"name": "default", "columns": [{"field": "Name", "width": "20rem", "priority": "high"}]}],
                "selection_fields": ["Name"],
                "filter_defaults": {"Name": "Example"},
                "facets": [{"facet": "General", "title": "Catalog", "fields": ["Name"]}],
                "ui": {"hidden_fields": ["Name"], "hide_facets": ["Default"]}
            }],
            "relationships": [],
            "apps": [{"name": "catalog", "title": "Catalog", "main_entity": "Catalog", "package": {"name": "test"}, "ui": {"modules": ["catalog.module"], "controller_extensions": [{"target": "ObjectPage", "controller": "catalog.controller"}]}}],
            "value_lists": [{"id": "debaebd2-8b12-405d-a582-5565c83f0061", "name": "Status", "values": [{"code": "A", "label": "Active", "sort": 0}]}]
        });
        let result = build(&base, &project(&base)).unwrap();
        for key in [
            "fields",
            "list_views",
            "selection_fields",
            "filter_defaults",
            "ui",
            "primary_key",
            "draft_enabled",
        ] {
            assert_eq!(
                result["entities"][0][key], base["entities"][0][key],
                "{key}"
            );
        }
        assert_eq!(
            result["entities"][0]["facets"][0]["fields"],
            base["entities"][0]["facets"][0]["fields"]
        );
        assert_eq!(result["apps"], base["apps"]);
        assert_eq!(result["value_lists"], base["value_lists"]);
        assert_eq!(build(&result, &project(&result)).unwrap(), result);
    }
}
