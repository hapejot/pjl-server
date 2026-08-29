use std::ops::{Index, IndexMut};

use crate::error::StoreError;
use crate::key::EntityKey;
use odata::query::ODataQuery;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftAction {
    Edit,
    Activate,
    Prepare,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DraftAdministrativeData {
    #[serde(rename = "DraftUUID", default)]
    pub draft_uuid: Uuid,
    #[serde(rename = "DraftIsCreatedByMe", default)]
    pub draft_is_created_by_me: bool,
    #[serde(rename = "InProcessByUser", default)]
    pub in_process_by_user: String,
    #[serde(rename = "LastChangedByUser", default)]
    pub last_changed_by_user: String,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DraftMessage {
    pub target: String,
    pub message: String,
    pub code: String,

    #[serde(rename = "numericSeverity")]
    pub severity: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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
    pub fn new(id: Uuid, is_active_entity: bool) -> Self {
        let has_active_entity: bool = false;
        let has_draft_entity: bool = false;
        let draft_admin_data = None;
        let draft_messages = vec![];
        let fields = Map::new();

        Self {
            id,
            is_active_entity,
            has_active_entity,
            has_draft_entity,
            draft_admin_data,
            draft_messages,
            fields,
        }
    }

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
            .unwrap_or_else(|| false);
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
        let draft_messages = fields
            .remove("DraftMessages")
            .and_then(|x| serde_json::from_value(x).ok())
            .unwrap_or_else(|| vec![]);
        let r = DataRecord {
            id,
            is_active_entity,
            has_active_entity,
            has_draft_entity,
            draft_admin_data,
            draft_messages,
            fields: fields.clone(),
        };
        Ok(r)
    }

    pub fn get(&self, segment: &str) -> Option<Value> {
        match segment {
            "ID" => Some(json!(self.id)),
            x => self.fields.get(x).cloned(),
        }
    }

    pub fn touch(&mut self, s: &str) {
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

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn is_active_entity(&self) -> bool {
        self.is_active_entity
    }

    pub fn has_active_entity(&self) -> bool {
        self.has_active_entity
    }

    pub fn has_draft_entity(&self) -> bool {
        self.has_draft_entity
    }

    pub fn draft_admin_data(&self) -> Option<&DraftAdministrativeData> {
        self.draft_admin_data.as_ref()
    }

    pub fn draft_messages(&self) -> &[DraftMessage] {
        &self.draft_messages
    }

    pub(crate) fn activate(&mut self) -> Result<(), StoreError> {
        self.has_active_entity = false;
        self.is_active_entity = true;
        self.has_draft_entity = false;
        self.draft_admin_data = None;
        self.draft_messages.clear();

        Ok(())
    }

    pub(crate) fn merge(&mut self, patch: &Value) {
        let patch_map = patch.as_object().unwrap();
        for (k, v) in patch_map {
            *self.fields.entry(k.clone()).or_insert(Value::Null) = v.clone();
        }
    }

    pub fn edit(&mut self) {
        self.has_active_entity = true;
        self.is_active_entity = false;
        self.has_draft_entity = false;
        self.draft_admin_data = Some(DraftAdministrativeData {
            draft_uuid: Uuid::new_v4(),
            draft_is_created_by_me: true,
            in_process_by_user: "user".into(),
            last_changed_by_user: "user".into(),
        });
    }

    pub fn core_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("ID".into(), json!(self.id));
        for (k, v) in self.fields.iter() {
            m.insert(k.clone(), v.clone());
        }
        json!(m)
    }
}

impl From<&DataRecord> for EntityKey {
    fn from(value: &DataRecord) -> Self {
        EntityKey::new(value.id, value.is_active_entity)
    }
}
impl Index<&str> for DataRecord {
    type Output = Value;

    fn index(&self, index: &str) -> &Self::Output {
        match &self.fields.get(index) {
            Some(v) => v,
            None => &Value::Null,
        }
    }
}

impl IndexMut<&str> for DataRecord {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        if !self.fields.contains_key(index) {
            self.fields.insert(index.into(), Value::Null);
        }
        self.fields.get_mut(index).unwrap()
    }
}

#[derive(Debug, Default)]
pub struct Navigation {
    pub single: bool,
    pub from_entity: Option<String>,
    pub from_field: Option<String>,
    pub to_entity: Option<String>,
    pub to_field: Option<String>,
}
impl Navigation {
    pub fn new(single: bool) -> Self {
        Self {
            single,
            ..Default::default()
        }
    }

    pub fn from(&mut self, arg_1: &str, arg_2: &str) -> &mut Self {
        self.from_entity = Some(arg_1.into());
        self.from_field = Some(arg_2.into());
        self
    }

    pub fn to(&mut self, arg_1: &str, arg_2: &str) -> &mut Self {
        self.to_entity = Some(arg_1.into());
        self.to_field = Some(arg_2.into());
        self
    }
}

pub trait DataStore: Send + Sync {
    // -- Collections --
    fn get_collection(
        &self,
        set_name: &str,
        query: &ODataQuery,
    ) -> Result<Vec<DataRecord>, StoreError>;

    fn count(&self, set_name: &str, query: &ODataQuery) -> usize;

    // -- Single Entity CRUD --
    fn read_entity(&self, set_name: &str, key: &EntityKey) -> Result<DataRecord, StoreError>;

    fn create_entity(&self, set_name: &str, data: &Value) -> Result<DataRecord, StoreError>;

    fn patch_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        patch: &Value,
    ) -> Result<Value, StoreError>;

    fn delete_entity(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError>;

    // -- Sibling Entity (draft <-> active) --
    fn read_sibling_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<DataRecord, StoreError>;

    // -- Property --
    fn get_property(
        &self,
        set_name: &str,
        key: &EntityKey,
        property: &str,
    ) -> Result<Value, StoreError>;

    // -- Persistence --
    fn commit(&self);

    // -- Default-Methoden (kein Implementierungszwang) --
    fn update_entities(
        &self,
        set_name: &str,
        updates: &[(EntityKey, Value)],
    ) -> Result<(), StoreError> {
        for (key, patch) in updates {
            self.patch_entity(set_name, key, patch)?;
        }
        Ok(())
    }

    fn seed_records(&self, set_name: &str, records: Vec<Value>) {
        for record in records {
            let _ = self.create_entity(set_name, &record);
        }
    }

    fn navigation(&mut self, single: bool) -> &mut Navigation;
}

/// Optionale Debug-Erweiterung. Bewusst getrennt vom Kern-Trait, damit
/// eine spaetere Postgres-Implementierung nicht gezwungen ist, einen
/// ungefilterten Full-Table-Scan anzubieten.
#[cfg(any(test, feature = "debug-tools"))]
pub trait DataStoreDebugExt: DataStore {
    fn get_records(&self, set_name: &str) -> Vec<Value>;
}
