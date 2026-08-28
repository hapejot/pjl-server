use crate::datastore::{DataRecord, DataStore};
use crate::error::StoreError;
use crate::filter_eval::evaluate_filter;
use crate::key::EntityKey;
use crate::metadata::EntityMetadata;
use crate::DraftActionExt;
use odata::query::ODataQuery;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;
use tracing::{info, trace};
use uuid::Uuid;

pub struct YamlStore {
    root: PathBuf,
    pub metadata: EntityMetadata,
    current_change: RwLock<Option<Uuid>>,
    // cache: RwLock<HashMap<String, HashMap<Uuid, DataRecord>>>,
}

#[allow(dead_code)]
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
#[allow(dead_code)]
fn merge_json(existing: &mut Value, patch: &Value) {
    match (existing, patch) {
        (Value::Object(existing_map), Value::Object(patch_map)) => {
            for (k, v) in patch_map {
                merge_json(existing_map.entry(k).or_insert(Value::Null), v);
            }
        }
        (existing_slot, patch_value) => {
            *existing_slot = patch_value.clone();
        }
    }
}

impl DraftActionExt for YamlStore {
    fn draft_edit(&self, set_name: &str, active_key: &EntityKey) -> Result<Value, StoreError> {
        // let draft_key = active_key.sibling(); // is_active_entity = false
        let mut draft_data = self.read_entity(set_name, active_key)?;
        draft_data.edit();
        self.write_record(set_name, draft_data.clone())?;

        Ok(json!(draft_data))
    }

    fn draft_activate(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        info!("activate draft");
        let path = self.path_for(set_name, key);
        let mut draft = load_record_from_file(path)?;
        draft.activate()?;
        self.write_record(set_name, draft.clone())?;
        self.remove_record(set_name, key)?;
        Ok(json!(draft))
    }

    fn draft_prepare(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        let path = self.path_for(set_name, key);
        let draft = load_record_from_file(path)?;
        Ok(json!(draft))
    }
}

impl YamlStore {
    pub fn open(root: impl Into<PathBuf>, metadata: EntityMetadata) -> Result<Self, StoreError> {
        let root = root.into();
        let store = Self {
            root,
            metadata,
            current_change: RwLock::new(None),
            // cache: RwLock::new(HashMap::new()),
        };
        // store.load_all()?;
        Ok(store)
    }

    pub fn get_change(&self) -> Uuid {
        let mut c = self.current_change.write().unwrap();
        match *c {
            Some(x) => x,
            None => {
                let new_change = Uuid::new_v4();
                *c = Some(new_change);
                new_change
            }
        }
    }

    fn load_records(&self, set_name: &str) -> Result<Vec<DataRecord>, StoreError> {
        // erst alle Drafts, dann alle Active laden, die nicht schon in Drafts sind.
        let p = self.path_for_set(set_name, true);
        let mut result = collect_records(p)?;
        let p = self.path_for_set(set_name, false);
        for x in collect_records(p)? {
            if result.iter().any(|r| r.id() == x.id()) {
                continue;
            }
            result.push(x);
        }

        Ok(result)
    }

    fn path_for_set(&self, set_name: &str, draft: bool) -> PathBuf {
        let state = match draft {
            true => "draft",
            false => "active",
        };
        let p = self.root.join(set_name).join(state);
        if !p.exists() {
            let _create_dir_all = fs::create_dir_all(p.clone());
        }
        p
    }

    fn path_for(&self, set_name: &str, key: &EntityKey) -> PathBuf {
        let p = self.path_for_set(set_name, !key.is_active_entity);
        p.join(format!("{}.yaml", key.storage_id()))
    }

    fn write_record(&self, set_name: &str, record: DataRecord) -> Result<(), StoreError> {
        let key: EntityKey = EntityKey::from(&record);
        let path = self.path_for(set_name, &key);
        let yaml =
            serde_yaml::to_string(&record).map_err(|e| StoreError::Serialize(e.to_string()))?;
        dbg!(&record);
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

    #[allow(dead_code)]
    fn enrich_with_technical_fields(
        &self,
        _set_name: &str,
        _key: &EntityKey,
        value: &Value,
    ) -> Value {
        let out = value.clone();
        out
    }
}

fn collect_records(p: PathBuf) -> Result<Vec<DataRecord>, StoreError> {
    let mut result = vec![];
    for entry in fs::read_dir(&p)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        let record = load_record_from_file(path)?;
        result.push(record);
    }
    Ok(result)
}

fn load_record_from_file(path: PathBuf) -> Result<DataRecord, StoreError> {
    let content = fs::read_to_string(&path)?;
    let record: DataRecord = serde_yaml::from_str(&content)
        .map_err(|e| StoreError::Deserialize(format!("{path:?}: {e}")))?;
    Ok(record)
}

impl DataStore for YamlStore {
    fn get_collection(
        &self,
        set_name: &str,
        query: &ODataQuery,
    ) -> Result<Vec<DataRecord>, StoreError> {
        info!("selected fields: {:?}", query.select);
        let mut records: Vec<DataRecord> = self
            .load_records(set_name)?
            .iter()
            .cloned()
            .map(|mut r| {
                for s in query.select.iter() {
                    r.touch(s);
                }
                r
            })
            .collect();

        if let Some(filter) = &query.filter {
            records.retain(|r| evaluate_filter(r, filter));
        }

        // for item in query.orderby.iter().rev() {
        //     records.sort_by(|a, b| {
        //         let (va, vb) = (
        //             resolve_property(a, &item.property),
        //             resolve_property(b, &item.property),
        //         );
        //         let ord = compare_json_for_sort(&va, &vb);
        //         if item.direction == SortDirection::Desc {
        //             ord.reverse()
        //         } else {
        //             ord
        //         }
        //     });
        // }

        // let skip = query.skip.unwrap_or(0) as usize;
        // let mut records: Vec<Value> = records.into_iter().skip(skip).collect();
        // if let Some(top) = query.top {
        //     records.truncate(top as usize);
        // }

        // // $expand VOR $select ausfuehren, damit die Navigation Property noch
        // // im vollen Record steckt.
        // for expand_item in &query.expand {
        //     let nav_def = self
        //         .metadata
        //         .lookup_navigation(set_name, &expand_item.navigation_property)?;

        //     for record in records.iter_mut() {
        //         let own_key = self.entity_key_of(set_name, record)?;
        //         let nested_parent = ParentKey {
        //             set_name: set_name.to_string(),
        //             key: own_key,
        //             navigation_property: expand_item.navigation_property.clone(),
        //         };
        //         let nested = self.get_collection(
        //             &nav_def.target_set,
        //             &expand_item.nested_query,
        //             Some(&nested_parent),
        //         )?;
        //         if let Value::Object(map) = record {
        //             map.insert(expand_item.navigation_property.clone(), nested);
        //         }
        //     }
        // }

        // if let Some(select) = &query.select {
        //     records = records.iter().map(|r| project_fields(r, select)).collect();
        // }

        Ok(records)
    }

    fn count(&self, set_name: &str, query: &ODataQuery) -> usize {
        // Einfachste Variante fuer den Prototyp: Collection ohne
        // top/skip/select/expand aufbauen und zaehlen.
        let mut q = query.clone();
        q.top = None;
        q.skip = None;
        q.expand.clear();
        q.select = vec![];
        match self.get_collection(set_name, &q) {
            Ok(items) => items.len(),
            _ => 0,
        }
    }

    fn read_entity(&self, set_name: &str, key: &EntityKey) -> Result<DataRecord, StoreError> {
        let p = self.path_for(set_name, key);
        let content = fs::read_to_string(&p)
            .or_else(|_| Err(StoreError::NotFound(format!("{p:?} for key {key:?}"))))?;
        let r = serde_yaml::from_str(&content)
            .or_else(|e| Err(StoreError::Deserialize(format!("{e}"))))
            .and_then(|x| DataRecord::from_json(&x))?;
        Ok(r)
    }

    fn create_entity(&self, set_name: &str, data: &Value) -> Result<DataRecord, StoreError> {
        let mut rec: DataRecord = DataRecord::from_json(data)?;
        rec.edit();
        self.write_record(set_name, rec.clone())?;
        Ok(rec)
    }

    fn patch_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        patch: &Value,
    ) -> Result<Value, StoreError> {
        trace!("patching yaml");
        let mut r = self.read_entity(set_name, key)?;
        r.merge(patch);
        dbg!(&r);
        self.write_record(set_name, r)?;
        Ok(patch.clone())
    }

    fn delete_entity(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        self.remove_record(set_name, key)?;
        Ok(())
    }

    fn get_property(
        &self,
        set_name: &str,
        key: &EntityKey,
        property: &str,
    ) -> Result<Value, StoreError> {
        let entity = self.read_entity(set_name, key)?;
        entity.get(property).ok_or_else(|| {
            StoreError::NotFound(format!("Property '{property}' auf {set_name}/{key:?}"))
        })
    }

    fn commit(&self) {}

    fn read_sibling_entity(
        &self,
        _set_name: &str,
        _key: &EntityKey,
    ) -> Result<DataRecord, StoreError> {
        todo!()
    }
}

#[allow(dead_code)]
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
