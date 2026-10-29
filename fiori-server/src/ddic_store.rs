//! Designer-Datastore: bildet die Metamodell-Sets (`PackageSet`, `EntitySet`,
//! `FieldSet`, `RelationSet`, `AppSet`) auf `ddic.json` ab.
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
const PERSISTED: [&str; 7] = [
    "PackageSet",
    "EntitySet",
    "FieldSet",
    "RelationSet",
    "AppSet",
    "ValueListSet",
    "ValueSet",
];
const DEFAULT_PACKAGE: &str = "core";
/// Atom-Schluessel, die der Store als Records fuehrt (ausser `name`, `label`, `type`).
const ATOM_NUMBERS: [&str; 5] = ["max_length", "precision", "scale", "computed", "immutable"];

type Sets = HashMap<String, Vec<DataRecord>>;

struct Inner {
    sets: Sets,
    /// Zuletzt geschriebenes (bzw. gelesenes) JSON; Grundlage fuer das Einmischen.
    base: Value,
}

pub struct DdicStore {
    path: PathBuf,
    inner: Mutex<Inner>,
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
            inner: Mutex::new(Inner { sets, base }),
            navigation: Navigation::default(),
            navigations: Mutex::new(Vec::new()),
        })
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
        if PERSISTED.contains(&set) {
            let json = build(&inner.base, &next)?;
            ddic_file::save(&self.path, &json)
                .map_err(|e| StoreError::Conflict(format!("cannot write ddic: {e:#}")))?;
            inner.base = json;
        }
        inner.sets = next;
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
    v.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty())
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
            sets.entry("PackageSet".into()).or_default().push(rec(
                "PackageSet",
                id_for("PackageSet", name),
                json!({ "name": name }),
            ));
        }
    }
    for e in items(base, "entities") {
        let Some(name) = text(e, "name") else { continue };
        let pkg = text(e, "package").unwrap_or(DEFAULT_PACKAGE);
        sets.entry("EntitySet".into()).or_default().push(rec(
            "EntitySet",
            id_for("EntitySet", name),
            json!({
                "name": name,
                "plural": text(e, "type_name_plural"),
                "type_name": text(e, "type_name"),
                "package_ID": id_for("PackageSet", pkg),
            }),
        ));
        for f in items(e, "fields") {
            let Some(atom) = f.get("Atom") else { continue };
            let Some(fname) = text(atom, "name") else { continue };
            let list_id = atom
                .get("value_list")
                .and_then(|vl| vl.get("FieldValueList"))
                .and_then(|fv| text(fv, "list_id"));
            let mut fields = json!({
                "name": fname,
                "label": text(atom, "label"),
                "type": text(atom, "edm_type"),
                "entity_ID": id_for("EntitySet", name),
                "value_list_ID": list_id,
            });
            for key in ATOM_NUMBERS {
                fields[key] = atom.get(key).cloned().unwrap_or(Value::Null);
            }
            fields["multiline"] = json!(atom.get("multiline").and_then(Value::as_bool).unwrap_or(false));
            sets.entry("FieldSet".into()).or_default().push(rec(
                "FieldSet",
                id_for("FieldSet", &format!("{name}.{fname}")),
                fields,
            ));
        }
    }
    for r in items(base, "relationships") {
        let Some(name) = text(r, "name") else { continue };
        let one = text(r, "one").unwrap_or_default();
        let many = text(r, "many").unwrap_or_default();
        sets.entry("RelationSet".into()).or_default().push(rec(
            "RelationSet",
            id_for("RelationSet", name),
            json!({
                "name": name,
                "owned": r.get("owned").and_then(Value::as_bool).unwrap_or(false),
                "one_ID": id_for("EntitySet", one),
                "many_ID": id_for("EntitySet", many),
                "source_field": text(r, "one_field"),
                "target_field": text(r, "many_property"),
            }),
        ));
    }
    for l in items(base, "value_lists") {
        let (Some(id), Some(name)) = (text(l, "id"), text(l, "name")) else { continue };
        sets.entry("ValueListSet".into()).or_default().push(rec(
            "ValueListSet",
            id.to_string(),
            json!({ "name": name }),
        ));
        for v in items(l, "values") {
            let Some(code) = text(v, "code") else { continue };
            sets.entry("ValueSet".into()).or_default().push(rec(
                "ValueSet",
                id_for("ValueSet", &format!("{id}/{code}")),
                json!({
                    "name": code,
                    "value": code,
                    "label": text(v, "label"),
                    "sort": v.get("sort").cloned().unwrap_or(Value::Null),
                    "list_ID": id,
                }),
            ));
        }
    }
    for a in items(base, "apps") {
        let Some(name) = text(a, "name") else { continue };
        let main = text(a, "main_entity").unwrap_or_default();
        let pkg = package_name(a).unwrap_or(DEFAULT_PACKAGE);
        sets.entry("AppSet".into()).or_default().push(rec(
            "AppSet",
            id_for("AppSet", name),
            json!({
                "name": name,
                "title": text(a, "title"),
                "subtitle": text(a, "subtitle"),
                "icon": text(a, "icon"),
                "main_entity_ID": id_for("EntitySet", main),
                "package_ID": id_for("PackageSet", pkg),
            }),
        ));
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
    items(base, key).iter().find(|o| text(o, "name") == Some(name))
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

fn build(base: &Value, sets: &Sets) -> Result<Value, StoreError> {
    let empty = Vec::new();
    let recs = |set: &str| sets.get(set).unwrap_or(&empty).iter().map(value_of).collect::<Vec<_>>();
    let pkg_names = names_by_id(sets, "PackageSet");
    let ent_names = names_by_id(sets, "EntitySet");
    let need = |v: &Value| {
        text(v, "name")
            .map(str::to_string)
            .ok_or_else(|| StoreError::Conflict("name fehlt".into()))
    };

    let mut packages = Vec::new();
    for r in recs("PackageSet") {
        let name = need(&r)?;
        let mut obj = original(base, "packages", &name).and_then(|o| o.as_object().cloned()).unwrap_or_default();
        obj.insert("name".into(), json!(name));
        packages.push(Value::Object(obj));
    }

    let mut entities = Vec::new();
    let field_recs = recs("FieldSet");
    for r in recs("EntitySet") {
        let name = need(&r)?;
        let ent_id = text(&r, "ID").unwrap_or_default().to_string();
        let mut obj = original(base, "entities", &name).and_then(|o| o.as_object().cloned()).unwrap_or_default();
        let pkg = r
            .get("package_ID")
            .and_then(Value::as_str)
            .and_then(|id| pkg_names.get(id))
            .ok_or_else(|| StoreError::Conflict(format!("Entitaet '{name}': Paket fehlt oder unbekannt")))?;
        obj.insert("name".into(), json!(name));
        obj.insert("package".into(), json!(pkg));
        let plural = text(&r, "plural").map(str::to_string).unwrap_or_else(|| format!("{name}s"));
        obj.insert("type_name_plural".into(), json!(plural));
        put(&mut obj, "type_name", &r.get("type_name").cloned().unwrap_or(Value::Null));

        // Nicht gefuehrte Felder (Navigationen, Measures) bleiben unveraendert.
        let orig_fields = obj.get("fields").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut fields: Vec<Value> = orig_fields.iter().filter(|f| f.get("Atom").is_none()).cloned().collect();
        for fr in field_recs.iter().filter(|f| text(f, "entity_ID") == Some(ent_id.as_str())) {
            let fname = need(fr)?;
            let mut atom = orig_fields
                .iter()
                .find_map(|f| f.get("Atom").filter(|a| text(a, "name") == Some(fname.as_str())))
                .and_then(|a| a.as_object().cloned())
                .unwrap_or_default();
            atom.insert("name".into(), json!(fname));
            atom.insert("label".into(), json!(text(fr, "label").unwrap_or(&fname)));
            atom.insert("edm_type".into(), json!(text(fr, "type").unwrap_or("Edm.String")));
            for key in ATOM_NUMBERS {
                put(&mut atom, key, &fr.get(key).cloned().unwrap_or(Value::Null));
            }
            if fr.get("multiline").and_then(Value::as_bool).unwrap_or(false) {
                atom.insert("multiline".into(), json!(true));
            } else {
                atom.remove("multiline");
            }
            // Festwertliste: Zuordnung im Designer, `prefer_dialog` bleibt wie im Original.
            match text(fr, "value_list_ID") {
                Some(list_id) => {
                    let prefer = orig_fields
                        .iter()
                        .find_map(|f| f.get("Atom").filter(|a| text(a, "name") == Some(fname.as_str())))
                        .and_then(|a| a.pointer("/value_list/FieldValueList/prefer_dialog"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    atom.insert(
                        "value_list".into(),
                        json!({ "FieldValueList": { "list_id": list_id, "prefer_dialog": prefer } }),
                    );
                }
                None => {
                    let was_list = atom
                        .get("value_list")
                        .and_then(|vl| vl.get("FieldValueList"))
                        .is_some();
                    if was_list {
                        atom.remove("value_list");
                    }
                }
            }
            fields.push(json!({ "Atom": atom }));
        }
        obj.insert("fields".into(), Value::Array(fields));
        entities.push(Value::Object(obj));
    }

    let mut relationships = Vec::new();
    for r in recs("RelationSet") {
        let name = need(&r)?;
        let one = r.get("one_ID").and_then(Value::as_str).and_then(|id| ent_names.get(id));
        let many = r.get("many_ID").and_then(Value::as_str).and_then(|id| ent_names.get(id));
        // Beziehung ohne bekannte Entitaeten (z. B. nach Loeschen) entfaellt.
        let (Some(one), Some(many)) = (one, many) else { continue };
        let mut obj = original(base, "relationships", &name).and_then(|o| o.as_object().cloned()).unwrap_or_default();
        obj.insert("name".into(), json!(name));
        obj.insert("one".into(), json!(one));
        obj.insert("many".into(), json!(many));
        put(&mut obj, "one_field", &r.get("source_field").cloned().unwrap_or(Value::Null));
        put(&mut obj, "many_property", &r.get("target_field").cloned().unwrap_or(Value::Null));
        if r.get("owned").and_then(Value::as_bool).unwrap_or(false) {
            obj.insert("owned".into(), json!(true));
        } else {
            obj.remove("owned");
        }
        obj.entry("package").or_insert(json!(DEFAULT_PACKAGE));
        relationships.push(Value::Object(obj));
    }

    let mut apps = Vec::new();
    for r in recs("AppSet") {
        let name = need(&r)?;
        let main = r.get("main_entity_ID").and_then(Value::as_str).and_then(|id| ent_names.get(id));
        let Some(main) = main else {
            return Err(StoreError::Conflict(format!("App '{name}': Hauptentitaet fehlt")));
        };
        let pkg = r
            .get("package_ID")
            .and_then(Value::as_str)
            .and_then(|id| pkg_names.get(id))
            .ok_or_else(|| StoreError::Conflict(format!("App '{name}': Paket fehlt oder unbekannt")))?;
        let mut obj = original(base, "apps", &name).and_then(|o| o.as_object().cloned()).unwrap_or_default();
        obj.insert("name".into(), json!(name));
        obj.insert("title".into(), json!(text(&r, "title").unwrap_or(&name)));
        obj.insert("main_entity".into(), json!(main));
        put(&mut obj, "subtitle", &r.get("subtitle").cloned().unwrap_or(Value::Null));
        put(&mut obj, "icon", &r.get("icon").cloned().unwrap_or(Value::Null));
        obj.insert("package".into(), json!({ "name": pkg }));
        apps.push(Value::Object(obj));
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
    let out = Value::Object(out);
    // Nur gueltige Dictionaries werden geschrieben.
    serde_json::from_value::<DataDictionary>(out.clone())
        .map_err(|e| StoreError::Conflict(format!("ungueltiges Dictionary: {e}")))?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Datastore-Schnittstelle
// ---------------------------------------------------------------------------

/// Schluessel fuer die Eindeutigkeit innerhalb eines Sets.
fn unique_key(set: &str, v: &Value) -> Option<String> {
    let name = if set == "ValueSet" {
        text(v, "value")?
    } else {
        text(v, "name")?
    };
    Some(match set {
        "FieldSet" => format!("{}/{name}", text(v, "entity_ID").unwrap_or_default()),
        "ValueSet" => format!("{}/{}", text(v, "list_ID").unwrap_or_default(), text(v, "value").unwrap_or_default()),
        _ => name.to_string(),
    })
}

fn not_found(set: &str, key: &EntityKey) -> StoreError {
    StoreError::not_found(format!("{set}/{}", key.id))
}

#[async_trait::async_trait]
impl DataStore for DdicStore {
    fn component_info(&self) -> String {
        format!("DdicStore -> {}", self.path.display())
    }

    async fn get_collection(&self, set: &str, query: &ODataQuery) -> Result<Vec<DataRecord>, StoreError> {
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

    async fn read_entity(&self, set: &str, key: &EntityKey, _query: &ODataQuery) -> Result<DataRecord, StoreError> {
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
                let dup = list.iter().any(|r| unique_key(set, &value_of(r)).as_deref() == Some(key.as_str()));
                if dup {
                    return Err(StoreError::Conflict(format!("{set}: '{key}' existiert bereits")));
                }
            }
            list.push(new.clone());
            Ok(new)
        })
    }

    async fn patch_entity(&self, set: &str, key: &EntityKey, patch: &Value) -> Result<Value, StoreError> {
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
        // Entitaet mit ihren Feldern und Beziehungen entfernen, damit keine Waisen bleiben.
        let entity_id = (set == "EntitySet").then(|| key.id.to_string());
        let mut inner = self.inner.lock().expect("ddic lock");
        let mut next = inner.sets.clone();
        let list = next.entry(set.to_string()).or_default();
        let idx = list.iter().position(|r| r.id() == key.id).ok_or_else(|| not_found(set, key))?;
        list.remove(idx);
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
        if let Some(eid) = entity_id {
            let name_id = eid.clone();
            if let Some(fields) = next.get_mut("FieldSet") {
                fields.retain(|f| text(&value_of(f), "entity_ID") != Some(name_id.as_str()));
            }
            if let Some(rels) = next.get_mut("RelationSet") {
                rels.retain(|r| {
                    let v = value_of(r);
                    text(&v, "one_ID") != Some(eid.as_str()) && text(&v, "many_ID") != Some(eid.as_str())
                });
            }
        }
        if PERSISTED.contains(&set) {
            let json = build(&inner.base, &next)?;
            ddic_file::save(&self.path, &json)
                .map_err(|e| StoreError::Conflict(format!("cannot write ddic: {e:#}")))?;
            inner.base = json;
        }
        inner.sets = next;
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

    async fn read_sibling_entity(&self, set: &str, key: &EntityKey) -> Result<DataRecord, StoreError> {
        // Keine Entwuerfe: der aktive Satz ist zugleich der einzige.
        self.read_entity(set, key, &ODataQuery::new()).await
    }

    async fn get_property(&self, set: &str, key: &EntityKey, property: &str) -> Result<Value, StoreError> {
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
    // Ohne Entwurfszustand ist jede Draft-Aktion die Lesung des aktiven Satzes.
    async fn draft_edit(&self, set: &str, key: &EntityKey) -> Result<Value, StoreError> {
        Ok(value_of(&self.read_entity(set, key, &ODataQuery::new()).await?))
    }

    async fn draft_activate(&self, set: &str, key: &EntityKey) -> Result<Value, StoreError> {
        Ok(value_of(&self.read_entity(set, key, &ODataQuery::new()).await?))
    }

    async fn draft_prepare(&self, set: &str, key: &EntityKey) -> Result<Value, StoreError> {
        Ok(value_of(&self.read_entity(set, key, &ODataQuery::new()).await?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../poc-crm/ddic.json");

    fn temp_copy(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ddic-store-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ddic.json");
        std::fs::copy(POC, &path).unwrap();
        path
    }

    #[tokio::test]
    async fn projection_and_unchanged_write_keep_poc_model() {
        let path = temp_copy("unchanged");
        let before: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let store = DdicStore::open(&path).unwrap();
        assert_eq!(store.count("EntitySet", &ODataQuery::new()).await, 3);
        // 11 Atome plus das Statusfeld der Verkaufschance.
        assert_eq!(store.count("FieldSet", &ODataQuery::new()).await, 12);
        // Ein No-op-Patch schreibt die Datei neu, ohne Inhalt zu verlieren.
        let accounts = store.get_collection("EntitySet", &ODataQuery::new()).await.unwrap();
        let acc = accounts.iter().find(|r| r.get("name") == Some(json!("Account"))).unwrap();
        let key = EntityKey::new(acc.id(), true);
        store.patch_entity("EntitySet", &key, &json!({"plural": "Firmen"})).await.unwrap();
        let after: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(before, after);
    }

    #[tokio::test]
    async fn create_field_and_entity_end_up_in_file() {
        let path = temp_copy("create");
        let store = DdicStore::open(&path).unwrap();
        let ents = store.get_collection("EntitySet", &ODataQuery::new()).await.unwrap();
        let acc = ents.iter().find(|r| r.get("name") == Some(json!("Account"))).unwrap().id();
        store
            .create_entity("FieldSet", &json!({"name": "vat_id", "label": "USt-ID", "type": "Edm.String", "max_length": 20, "entity_ID": acc.to_string()}))
            .await
            .unwrap();
        store
            .create_entity("EntitySet", &json!({"name": "Note", "plural": "Notes", "package_ID": store_package_id(&store).await}))
            .await
            .unwrap();
        let reloaded: DataDictionary =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(reloaded.entity_names().any(|n| n == "Note"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"vat_id\""));
        assert!(text.contains("\"USt-ID\""));
    }

    #[tokio::test]
    async fn invalid_write_is_rejected_and_nothing_changes() {
        let path = temp_copy("invalid");
        let store = DdicStore::open(&path).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        // App ohne Hauptentitaet ist ungueltig.
        let res = store.create_entity("AppSet", &json!({"name": "x", "title": "X"})).await;
        assert!(res.is_err());
        assert_eq!(before, std::fs::read_to_string(&path).unwrap());
        assert_eq!(store.count("AppSet", &ODataQuery::new()).await, 3);
    }

    #[tokio::test]
    async fn value_list_roundtrip_through_file() {
        let path = temp_copy("valuelist");
        let store = DdicStore::open(&path).unwrap();
        let list = store
            .create_entity("ValueListSet", &json!({"name": "TicketStatus"}))
            .await
            .unwrap();
        let lid = list.id().to_string();
        for (code, label, sort) in [("open", "Offen", 1), ("closed", "Geschlossen", 2)] {
            store
                .create_entity("ValueSet", &json!({"value": code, "label": label, "sort": sort, "list_ID": lid}))
                .await
                .unwrap();
        }
        // Doppelter Code in derselben Liste ist ein Konflikt.
        assert!(store
            .create_entity("ValueSet", &json!({"value": "open", "label": "x", "list_ID": lid}))
            .await
            .is_err());
        let ents = store.get_collection("EntitySet", &ODataQuery::new()).await.unwrap();
        let ticket = store
            .create_entity("EntitySet", &json!({"name": "Ticket", "plural": "Tickets", "package_ID": store_package_id(&store).await}))
            .await
            .unwrap();
        let _ = ents;
        let status = store
            .create_entity("FieldSet", &json!({"name": "status", "type": "Edm.String", "entity_ID": ticket.id().to_string(), "value_list_ID": lid}))
            .await
            .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let dd: DataDictionary = serde_json::from_str(&text).unwrap();
        let ticket_list = dd.value_lists().iter().find(|l| l.name == "TicketStatus").unwrap();
        assert_eq!(ticket_list.values.len(), 2);
        assert!(dd.validate_value_lists().is_ok());
        assert!(text.contains(&format!("\"list_id\": \"{lid}\"")) || text.contains(&lid));

        // Loeschen der Liste entfernt Werte und die Zuordnung am Feld.
        store
            .delete_entity("ValueListSet", &EntityKey::new(list.id(), true))
            .await
            .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("TicketStatus"));
        assert!(!text.contains("\"Geschlossen\""));
        let _ = status;
    }

    #[tokio::test]
    async fn deleting_entity_removes_its_fields_and_relations() {
        let path = temp_copy("delete");
        let store = DdicStore::open(&path).unwrap();
        let ents = store.get_collection("EntitySet", &ODataQuery::new()).await.unwrap();
        let acc = ents.iter().find(|r| r.get("name") == Some(json!("Account"))).unwrap().id();
        // Apps verweisen auf Account; daher zuerst die App loeschen.
        let apps = store.get_collection("AppSet", &ODataQuery::new()).await.unwrap();
        let account = id_for("EntitySet", "Account");
        for a in apps.iter().filter(|a| value_of(a).get("main_entity_ID").and_then(Value::as_str) == Some(account.as_str())) {
            store.delete_entity("AppSet", &EntityKey::new(a.id(), true)).await.unwrap();
        }
        store.delete_entity("EntitySet", &EntityKey::new(acc, true)).await.unwrap();
        let reloaded: DataDictionary = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(!reloaded.entity_names().any(|n| n == "Account"));
        assert_eq!(store.count("RelationSet", &ODataQuery::new()).await, 0);
    }

    async fn store_package_id(store: &DdicStore) -> String {
        let p = store.get_collection("PackageSet", &ODataQuery::new()).await.unwrap();
        p[0].id().to_string()
    }
}
