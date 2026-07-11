use crate::datastore::{DataStore, DraftAction};
use crate::error::StoreError;
use crate::filter_eval::{compare_json_for_sort, evaluate_filter, resolve_property};
use crate::key::{EntityKey, ParentKey};
use crate::metadata::EntityMetadata;
use crate::query::{ODataQuery, SortDirection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;

#[derive(Serialize, Deserialize)]
struct Record {
    key: EntityKey,
    data: Value,
}

pub struct YamlStore {
    root: PathBuf,
    pub metadata: EntityMetadata,
    // set_name -> EntityKey -> Value  (In-Memory-Index fuer schnelle Reads/Queries)
    cache: RwLock<HashMap<String, HashMap<EntityKey, Value>>>,
}

fn json_value_to_key_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Rekursiver Merge: Objekte werden Feld fuer Feld gemergt, alles andere
/// (Arrays, Skalare) wird vom Patch komplett ueberschrieben.
fn merge_json(existing: &mut Value, patch: &Value) {
    match (existing, patch) {
        (Value::Object(existing_map), Value::Object(patch_map)) => {
            for (k, v) in patch_map {
                merge_json(
                    existing_map.entry(k.clone()).or_insert(Value::Null),
                    v,
                );
            }
        }
        (existing_slot, patch_value) => {
            *existing_slot = patch_value.clone();
        }
    }
}

impl YamlStore {
    pub fn open(root: impl Into<PathBuf>, metadata: EntityMetadata) -> Result<Self, StoreError> {
        let root = root.into();
        let store = Self {
            root,
            metadata,
            cache: RwLock::new(HashMap::new()),
        };
        store.load_all()?;
        Ok(store)
    }

    fn load_all(&self) -> Result<(), StoreError> {
        let mut cache = self.cache.write().unwrap();
        if !self.root.exists() {
            return Ok(());
        }
        for set_dir in fs::read_dir(&self.root)? {
            let set_dir = set_dir?;
            if !set_dir.file_type()?.is_dir() {
                continue;
            }
            let set_name = set_dir.file_name().to_string_lossy().to_string();
            let mut set_cache = HashMap::new();

            for state in ["active", "draft"] {
                let state_dir = set_dir.path().join(state);
                if !state_dir.exists() {
                    continue;
                }

                for entry in fs::read_dir(&state_dir)? {
                    let path = entry?.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
                        continue;
                    }
                    let content = fs::read_to_string(&path)?;
                    let record: Record = serde_yaml::from_str(&content)
                        .map_err(|e| StoreError::Deserialize(format!("{path:?}: {e}")))?;
                    set_cache.insert(record.key, record.data);
                }
            }
            cache.insert(set_name, set_cache);
        }
        Ok(())
    }

    fn path_for(&self, set_name: &str, key: &EntityKey) -> PathBuf {
        let state = if key.is_active_entity { "active" } else { "draft" };
        self.root
            .join(set_name)
            .join(state)
            .join(format!("{}.yaml", key.storage_id()))
    }

    /// Schreibt atomar: erst temp-Datei, dann rename (verhindert kaputte Dateien bei Crash).
    fn write_record(&self, set_name: &str, key: &EntityKey, data: &Value) -> Result<(), StoreError> {
        let path = self.path_for(set_name, key);
        fs::create_dir_all(path.parent().unwrap())?;

        let record = Record {
            key: key.clone(),
            data: data.clone(),
        };
        let yaml = serde_yaml::to_string(&record)
            .map_err(|e| StoreError::Serialize(e.to_string()))?;

        let tmp_path = path.with_extension("yaml.tmp");
        fs::write(&tmp_path, yaml)?;
        fs::rename(&tmp_path, &path)?;
        Ok(())
    }

    fn remove_record(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        let path = self.path_for(set_name, key);
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Extrahiert den EntityKey aus einem Record anhand der registrierten Key-Felder.
    pub fn entity_key_of(&self, set_name: &str, record: &Value) -> Result<EntityKey, StoreError> {
        let fields = self.metadata.key_fields(set_name)?;
        let mut map = std::collections::BTreeMap::new();
        for f in fields {
            let v = record.get(f).ok_or_else(|| {
                StoreError::Metadata(format!("Key-Feld '{f}' fehlt in Record von '{set_name}'"))
            })?;
            map.insert(f.clone(), json_value_to_key_string(v));
        }
        let is_active_entity = record
            .get("IsActiveEntity")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        Ok(EntityKey {
            fields: map,
            is_active_entity,
        })
    }

    fn get_from_cache(&self, set_name: &str, key: &EntityKey) -> Option<Value> {
        self.cache
            .read()
            .unwrap()
            .get(set_name)
            .and_then(|s| s.get(key))
            .cloned()
    }

    fn draft_edit(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        let active_key = EntityKey {
            fields: key.fields.clone(),
            is_active_entity: true,
        };
        let active_data = self.get_from_cache(set_name, &active_key).ok_or_else(|| {
            StoreError::NotFound(format!("Aktive Entity {set_name}/{active_key:?} nicht gefunden"))
        })?;

        let draft_key = active_key.sibling(); // is_active_entity = false
        let mut draft_data = active_data;
        if let Value::Object(map) = &mut draft_data {
            map.insert("IsActiveEntity".to_string(), Value::Bool(false));
            map.insert("HasActiveEntity".to_string(), Value::Bool(true));
        }

        self.write_record(set_name, &draft_key, &draft_data)?;
        self.cache
            .write()
            .unwrap()
            .entry(set_name.to_string())
            .or_default()
            .insert(draft_key, draft_data.clone());

        Ok(draft_data)
    }

    fn draft_activate(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        let draft_key = EntityKey {
            fields: key.fields.clone(),
            is_active_entity: false,
        };
        let mut draft_data = self.get_from_cache(set_name, &draft_key).ok_or_else(|| {
            StoreError::NotFound(format!("Draft {set_name}/{draft_key:?} nicht gefunden"))
        })?;

        let active_key = draft_key.sibling(); // is_active_entity = true
        if let Value::Object(map) = &mut draft_data {
            map.insert("IsActiveEntity".to_string(), Value::Bool(true));
            map.insert("HasActiveEntity".to_string(), Value::Bool(false));
        }

        self.write_record(set_name, &active_key, &draft_data)?;
        self.remove_record(set_name, &draft_key)?;

        let mut cache = self.cache.write().unwrap();
        let set_cache = cache.entry(set_name.to_string()).or_default();
        set_cache.remove(&draft_key);
        set_cache.insert(active_key, draft_data.clone());

        Ok(draft_data)
    }

    fn draft_prepare(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        // Prototyp-Verhalten: kein echtes Validierungsframework, gibt einfach
        // die aktuell adressierte Entity zurueck (Draft, falls vorhanden, sonst aktiv).
        // Hier waere der Ansatzpunkt fuer Determinations/Validierungen aus RAP.
        let draft_key = EntityKey {
            fields: key.fields.clone(),
            is_active_entity: false,
        };
        if let Some(data) = self.get_from_cache(set_name, &draft_key) {
            return Ok(data);
        }
        self.get_from_cache(set_name, key)
            .ok_or_else(|| StoreError::NotFound(format!("{set_name}/{key:?}")))
    }
}

impl DataStore for YamlStore {
    fn get_collection(
        &self,
        set_name: &str,
        query: &ODataQuery,
        parent: Option<&ParentKey>,
    ) -> Result<Value, StoreError> {
        let mut records: Vec<Value> = {
            let cache = self.cache.read().unwrap();
            cache
                .get(set_name)
                .map(|s| s.values().cloned().collect())
                .unwrap_or_default()
        };

        // Composition-Scoping: nur Kinder des angegebenen Parents.
        if let Some(parent) = parent {
            let nav_def = self
                .metadata
                .lookup_navigation(&parent.set_name, &parent.navigation_property)?;
            // Vereinfachung: Foreign Key referenziert genau EIN Feld des Parent-Keys.
            let parent_key_value = parent
                .key
                .fields
                .values()
                .next()
                .ok_or_else(|| StoreError::Metadata("Parent-Key ist leer".into()))?;

            records.retain(|r| {
                r.get(&nav_def.foreign_key_field)
                    .map(json_value_to_key_string)
                    .as_ref()
                    == Some(parent_key_value)
            });
        }

        if let Some(filter) = &query.filter {
            records.retain(|r| evaluate_filter(r, filter));
        }

        for item in query.orderby.iter().rev() {
            records.sort_by(|a, b| {
                let (va, vb) = (
                    resolve_property(a, &item.property),
                    resolve_property(b, &item.property),
                );
                let ord = compare_json_for_sort(&va, &vb);
                if item.direction == SortDirection::Desc {
                    ord.reverse()
                } else {
                    ord
                }
            });
        }

        let skip = query.skip.unwrap_or(0) as usize;
        let mut records: Vec<Value> = records.into_iter().skip(skip).collect();
        if let Some(top) = query.top {
            records.truncate(top as usize);
        }

        // $expand VOR $select ausfuehren, damit die Navigation Property noch
        // im vollen Record steckt.
        for expand_item in &query.expand {
            let nav_def = self
                .metadata
                .lookup_navigation(set_name, &expand_item.navigation_property)?;

            for record in records.iter_mut() {
                let own_key = self.entity_key_of(set_name, record)?;
                let nested_parent = ParentKey {
                    set_name: set_name.to_string(),
                    key: own_key,
                    navigation_property: expand_item.navigation_property.clone(),
                };
                let nested = self.get_collection(
                    &nav_def.target_set,
                    &expand_item.nested_query,
                    Some(&nested_parent),
                )?;
                if let Value::Object(map) = record {
                    map.insert(expand_item.navigation_property.clone(), nested);
                }
            }
        }

        if let Some(select) = &query.select {
            records = records
                .iter()
                .map(|r| project_fields(r, select))
                .collect();
        }

        Ok(Value::Array(records))
    }

    fn count(&self, set_name: &str, query: &ODataQuery, parent: Option<&ParentKey>) -> usize {
        // Einfachste Variante fuer den Prototyp: Collection ohne
        // top/skip/select/expand aufbauen und zaehlen.
        let mut q = query.clone();
        q.top = None;
        q.skip = None;
        q.expand.clear();
        q.select = None;
        match self.get_collection(set_name, &q, parent) {
            Ok(Value::Array(items)) => items.len(),
            _ => 0,
        }
    }

    fn read_entity(&self, set_name: &str, key: &EntityKey, _query: &ODataQuery) -> Result<Value, StoreError> {
        self.get_from_cache(set_name, key)
            .ok_or_else(|| StoreError::NotFound(format!("{set_name}/{key:?}")))
    }

    fn create_entity(
        &self,
        set_name: &str,
        data: &Value,
        _parent: Option<&ParentKey>,
    ) -> Result<Value, StoreError> {
        let key = self.entity_key_of(set_name, data)?;

        if self.get_from_cache(set_name, &key).is_some() {
            return Err(StoreError::Conflict(format!(
                "Entity {set_name}/{key:?} existiert bereits"
            )));
        }

        self.write_record(set_name, &key, data)?;

        self.cache
            .write()
            .unwrap()
            .entry(set_name.to_string())
            .or_default()
            .insert(key, data.clone());

        Ok(data.clone())
    }

    fn patch_entity(&self, set_name: &str, key: &EntityKey, patch: &Value) -> Result<Value, StoreError> {
        let mut cache = self.cache.write().unwrap();
        let set = cache.entry(set_name.to_string()).or_default();
        let existing = set
            .get_mut(key)
            .ok_or_else(|| StoreError::NotFound(format!("{set_name}/{key:?}")))?;

        merge_json(existing, patch);
        let updated = existing.clone();
        drop(cache);

        self.write_record(set_name, key, &updated)?;
        Ok(updated)
    }

    fn delete_entity(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        self.remove_record(set_name, key)?;
        let mut cache = self.cache.write().unwrap();
        if let Some(set) = cache.get_mut(set_name) {
            set.remove(key);
        }
        Ok(())
    }

    fn draft_action(&self, set_name: &str, key: &EntityKey, action: DraftAction) -> Result<Value, StoreError> {
        match action {
            DraftAction::Edit => self.draft_edit(set_name, key),
            DraftAction::Activate => self.draft_activate(set_name, key),
            DraftAction::Prepare => self.draft_prepare(set_name, key),
        }
    }

    fn read_sibling_entity(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        let sibling_key = key.sibling();
        self.get_from_cache(set_name, &sibling_key)
            .ok_or_else(|| StoreError::NotFound(format!("{set_name}/{sibling_key:?}")))
    }

    fn get_property(&self, set_name: &str, key: &EntityKey, property: &str) -> Result<Value, StoreError> {
        let entity = self.read_entity(set_name, key, &ODataQuery::default())?;
        entity
            .get(property)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("Property '{property}' auf {set_name}/{key:?}")))
    }

    fn commit(&self) {
        // Write-Through: jede Operation ist bereits beim Aufruf persistent
        // (siehe write_record/remove_record) -> hier bewusst No-Op.
        //
        // Falls spaeter auf Write-Behind umgestellt wird (Aenderungen erst
        // im Cache sammeln, dann gebuendelt schreiben - naeher am echten
        // OData-$batch-Verhalten), waere hier der eigentliche Flush-Punkt.
    }
}

fn project_fields(record: &Value, select: &[String]) -> Value {
    let Value::Object(map) = record else {
        return record.clone();
    };
    let projected: serde_json::Map<String, Value> = select
        .iter()
        .filter_map(|field| map.get(field).map(|v| (field.clone(), v.clone())))
        .collect();
    Value::Object(projected)
}

#[cfg(any(test, feature = "debug-tools"))]
impl crate::datastore::DataStoreDebugExt for YamlStore {
    fn get_records(&self, set_name: &str) -> Vec<Value> {
        self.cache
            .read()
            .unwrap()
            .get(set_name)
            .map(|s| s.values().cloned().collect())
            .unwrap_or_default()
    }
}
