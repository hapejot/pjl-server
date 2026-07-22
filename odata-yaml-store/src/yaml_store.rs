use crate::datastore::{DataStore, DraftAction};
use crate::error::StoreError;
use crate::filter_eval::evaluate_filter;
use crate::key::EntityKey;
use crate::metadata::EntityMetadata;
use odata::query::ODataQuery;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;
use tracing::info;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone)]
pub struct DraftAdministrativeData {
    #[serde(rename = "DraftUUID", default)]
    pub draft_uuid: Uuid,
    #[serde(rename = "DraftIsCreatedByMe", default)]
    pub draft_is_created_by_me: bool,
    #[serde(rename = "DraftInProcessByUser", default)]
    pub draft_in_process_by_user: String,
    #[serde(rename = "LastChangedByUser", default)]
    pub last_changed_by_user: String,
}
#[derive(Serialize, Deserialize, Clone)]
pub struct DraftMessage {
    pub target: String,
    pub message: String,
    pub code: String,

    #[serde(rename = "numericSeverity")]
    pub severity: usize,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DataRecord {
    #[serde(rename = "ID")]
    id: Uuid,
    #[serde(rename = "IsActiveEntity")]
    is_active_entity: bool,
    #[serde(rename = "HasActiveEntity", default)]
    has_active_entity: bool,
    #[serde(rename = "HasDraftEntity", default = "bool::default")]
    has_draft_entity: bool,
    #[serde(rename = "DraftAdministrativeData", default)]
    draft_admin_data: Option<DraftAdministrativeData>,
    #[serde(rename = "DraftMessages", default)]
    draft_messages: Vec<DraftMessage>,
    #[serde(flatten)]
    fields: Map<String, Value>,
}

impl DataRecord {
    pub fn from_json(value: &Value) -> Result<DataRecord, StoreError> {
        let mut fields = value.clone();
        let fields = fields
            .as_object_mut()
            .ok_or_else(|| StoreError::Serialize("as object".into()))?;
        let id = fields
            .remove("ID")
            .and_then(|x| x.as_str().map(|s| s.to_string()))
            .and_then(|x| Uuid::parse_str(&x).ok())
            .unwrap_or_else(|| Uuid::new_v4());
        let is_active_entity = fields
            .remove("IsActiveEntity")
            .and_then(|x| x.as_bool())
            .unwrap_or_else(|| true);
        let has_active_entity = fields
            .remove("HasActiveEntity")
            .and_then(|x| x.as_bool())
            .unwrap_or_else(|| false);
        let draft_admin_data = fields
            .remove("DraftAdministrativeData")
            .and_then(|x| serde_json::from_value(x).ok());
        let has_draft_entity = fields
            .remove("HasDraftEntity")
            .and_then(|x| x.as_bool())
            .unwrap_or_else(|| false);
        let fields = fields.clone();
        let r = DataRecord {
            id,
            is_active_entity,
            has_active_entity,
            has_draft_entity,
            draft_admin_data,
            draft_messages: vec![],
            fields,
        };
        Ok(r)
    }

    pub(crate) fn get(&self, segment: &str) -> Option<Value> {
        match segment {
            "ID" => Some(json!(self.id)),
            x => self.fields.get(x).cloned(),
        }
    }

    fn touch(&mut self, s: &str) {
        if s == "ID"
            || s == "IsActiveEntity"
            || s == "HasDraftEntity"
            || s == "HasActiveEntity"
            || s == "DraftMessages"
            || s == "DraftAdministrativeData"
        {
            return;
        }
        if !self.fields.contains_key(s) {
            self.fields.insert(s.to_string(), json!(format!("<{}>", s)));
        }
    }
}

impl From<DataRecord> for EntityKey {
    fn from(value: DataRecord) -> Self {
        EntityKey::new(value.id, value.is_active_entity)
    }
}

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
                merge_json(existing_map.entry(k.clone()).or_insert(Value::Null), v);
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
        let p = self.path_for_set(set_name, false);
        let mut result = collect_records(p)?;
        let p = self.path_for_set(set_name, true);
        result.extend(collect_records(p)?);

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
        let key: EntityKey = EntityKey::new(record.id, record.is_active_entity);
        let path = self.path_for(set_name, &key);
        let yaml =
            serde_yaml::to_string(&record).map_err(|e| StoreError::Serialize(e.to_string()))?;
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

    fn draft_edit(&self, set_name: &str, active_key: &EntityKey) -> Result<Value, StoreError> {
        let draft_key = active_key.sibling(); // is_active_entity = false
        let draft_data = DataRecord {
            id: active_key.id,
            is_active_entity: active_key.is_active_entity,
            has_active_entity: true,
            has_draft_entity: false,
            draft_messages: vec![],
            draft_admin_data: Some(DraftAdministrativeData {
                draft_uuid: self.get_change(),
                draft_is_created_by_me: true,
                draft_in_process_by_user: "user".into(),
                last_changed_by_user: "user".into(),
            }),
            fields: Map::new(),
        };

        Ok(json!([set_name, draft_key, draft_data]))
    }

    fn draft_activate(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        Ok(json!([set_name, key]))
    }

    fn draft_prepare(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        let draft_key = key.sibling();
        Ok(json!([set_name, draft_key]))
    }
}

fn collect_records(p: PathBuf) -> Result<Vec<DataRecord>, StoreError> {
    let mut result = vec![];
    for entry in fs::read_dir(&p)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        let content = fs::read_to_string(&path)?;
        let record: DataRecord = serde_yaml::from_str(&content)
            .map_err(|e| StoreError::Deserialize(format!("{path:?}: {e}")))?;
        result.push(record);
    }
    Ok(result)
}

impl DataStore for YamlStore {
    fn get_collection(&self, set_name: &str, query: &ODataQuery) -> Result<Value, StoreError> {
        info!("selected fields: {:?}", query.select);
        let mut records: Vec<DataRecord> = self
            .load_records(set_name)?
            .iter()
            .cloned()
            .map(|mut r| {
                for s in query.select.iter().flatten() {
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

        Ok(json!(records))
    }

    fn count(&self, set_name: &str, query: &ODataQuery) -> usize {
        // Einfachste Variante fuer den Prototyp: Collection ohne
        // top/skip/select/expand aufbauen und zaehlen.
        let mut q = query.clone();
        q.top = None;
        q.skip = None;
        q.expand.clear();
        q.select = None;
        match self.get_collection(set_name, &q) {
            Ok(Value::Array(items)) => items.len(),
            _ => 0,
        }
    }

    fn read_entity(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        let p = self.path_for(set_name, key);
        let content = fs::read_to_string(&p)
            .or_else(|_| Err(StoreError::NotFound(format!("{p:?} for key {key:?}"))))?;
        let r = serde_yaml::from_str(&content)
            .or_else(|e| Err(StoreError::Deserialize(format!("{e}"))))
            .and_then(|x| DataRecord::from_json(&x))?;
        Ok(json!(r))
    }

    fn create_entity(&self, set_name: &str, data: &Value) -> Result<Value, StoreError> {
        // let key = EntityKey::new_from_json(&data);
        let mut rec: DataRecord = DataRecord::from_json(data)?;
        rec.has_active_entity = false;
        rec.is_active_entity = false;
        rec.draft_admin_data = Some(DraftAdministrativeData {
            draft_uuid: self.get_change(),
            draft_is_created_by_me: true,
            draft_in_process_by_user: "user".into(),
            last_changed_by_user: "user".into(),
        });
        self.write_record(set_name, rec.clone())?;

        Ok(json!(rec))
    }

    fn patch_entity(
        &self,
        _set_name: &str,
        _key: &EntityKey,
        patch: &Value,
    ) -> Result<Value, StoreError> {
        Ok(patch.clone())
    }

    fn delete_entity(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        self.remove_record(set_name, key)?;
        Ok(())
    }

    fn draft_action(
        &self,
        set_name: &str,
        key: &EntityKey,
        action: DraftAction,
    ) -> Result<Value, StoreError> {
        match action {
            DraftAction::Edit => self.draft_edit(set_name, key),
            DraftAction::Activate => self.draft_activate(set_name, key),
            DraftAction::Prepare => self.draft_prepare(set_name, key),
        }
    }

    fn get_property(
        &self,
        set_name: &str,
        key: &EntityKey,
        property: &str,
    ) -> Result<Value, StoreError> {
        let entity = self.read_entity(set_name, key)?;
        entity.get(property).cloned().ok_or_else(|| {
            StoreError::NotFound(format!("Property '{property}' auf {set_name}/{key:?}"))
        })
    }

    fn commit(&self) {}

    fn read_sibling_entity(&self, _set_name: &str, _key: &EntityKey) -> Result<Value, StoreError> {
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
