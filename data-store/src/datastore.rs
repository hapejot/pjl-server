use std::ops::{Index, IndexMut};

use crate::error::StoreError;
use crate::key::EntityKey;
use odata::query::ODataQuery;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

tokio::task_local! {
    /// Benutzer der laufenden Anfrage, unter dem Entwuerfe angelegt und bearbeitet
    /// werden (`InProcessByUser`, `LastChangedByUser`). `None` ausserhalb einer Anfrage.
    pub static CURRENT_PRINCIPAL: Option<String>;
}

/// Ersatz, wenn keine Anfrage mit Benutzer laeuft (Seeds, Start). Entspricht dem
/// Default User der Benutzerverwaltung.
pub const FALLBACK_PRINCIPAL: &str = "none@localhost";

/// Benutzer fuer Entwurfsverwaltung: der Benutzer der Anfrage oder [`FALLBACK_PRINCIPAL`].
pub fn current_principal() -> String {
    CURRENT_PRINCIPAL
        .try_with(|u| u.clone())
        .ok()
        .flatten()
        .unwrap_or_else(|| FALLBACK_PRINCIPAL.to_string())
}

/// `DraftIsCreatedByMe` aus Sicht der laufenden Anfrage: der Entwurf liegt beim
/// Benutzer der Anfrage (`InProcessByUser`). Wird beim Lesen berechnet, nicht
/// gespeichert, weil der Wert je Anfrage verschieden ist.
pub fn draft_created_by_current_user(in_process_by_user: &str) -> bool {
    in_process_by_user == current_principal()
}

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
        // Fehlt die ID (Neuanlage), wird eine UUID vergeben. Eine vorhandene,
        // aber ungueltige ID ist ein Fehler: frueher wurde sie stillschweigend
        // durch eine zufaellige UUID ersetzt, bei jedem Lesen eine andere.
        let id = match fields.remove("ID") {
            None | Some(Value::Null) => Uuid::new_v4(),
            Some(Value::String(s)) => Uuid::parse_str(&s).map_err(|_| {
                StoreError::Serialize(format!("ID '{s}' ist keine gueltige UUID"))
            })?,
            Some(other) => {
                return Err(StoreError::Serialize(format!(
                    "ID muss eine UUID-Zeichenkette sein, erhalten: {other}"
                )))
            }
        };
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
            "IsActiveEntity" => Some(json!(self.is_active_entity)),
            "HasActiveEntity" => Some(json!(self.has_active_entity)),
            "HasDraftEntity" => Some(json!(self.has_draft_entity)),
            "DraftMessages" => Some(json!(self.draft_messages)),
            "DraftAdministrativeData_ID" => Some(
                self.draft_admin_data
                    .as_ref()
                    .map(|x| json!(x.draft_uuid))
                    .unwrap_or(Value::Null),
            ),
            "DraftAdministrativeData" => Some(json!(self.draft_admin_data)),
            x => self.fields.get(x).cloned(),
        }
    }

    /// Entfernt ein Feld, das nur intern (z. B. fuer `$expand`) geladen wurde.
    pub fn remove_field(&mut self, name: &str) {
        self.fields.remove(name);
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

    pub fn edit(&mut self) {
        self.has_active_entity = true;
        self.is_active_entity = false;
        self.has_draft_entity = false;
        self.draft_admin_data = Some(DraftAdministrativeData {
            draft_uuid: Uuid::new_v4(),
            draft_is_created_by_me: true,
            in_process_by_user: current_principal(),
            last_changed_by_user: current_principal(),
        });
    }

    /// Neu angelegter Draft - im Gegensatz zu `edit()` gibt es noch keinen aktiven Satz.
    pub fn create_draft(&mut self) {
        self.edit();
        self.has_active_entity = false;
    }

    /// Activate a draft: transform draft into active entity by clearing draft-related fields
    pub fn activate(&mut self) -> Result<(), StoreError> {
        self.has_active_entity = false;
        self.is_active_entity = true;
        self.has_draft_entity = false;
        self.draft_admin_data = None;
        self.draft_messages.clear();
        Ok(())
    }

    /// Merge patch data into this record's fields
    pub fn merge(&mut self, patch: &Value) {
        if let Some(patch_map) = patch.as_object() {
            for (k, v) in patch_map {
                *self.fields.entry(k.clone()).or_insert(Value::Null) = v.clone();
            }
        }
    }

    pub fn core_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("ID".into(), json!(self.id));
        m.insert("IsActiveEntity".into(), json!(self.is_active_entity));
        m.insert("HasActiveEntity".into(), json!(self.has_active_entity));
        for (k, v) in self.fields.iter() {
            m.insert(k.clone(), v.clone());
        }
        json!(m)
    }

    pub fn set_draft_uuid(&mut self, draft_uid: Uuid) {
        if let Some(data) = self.draft_admin_data.as_mut() {
            data.draft_uuid = draft_uid;
        } else {
            self.draft_admin_data = Some(DraftAdministrativeData {
                draft_uuid: draft_uid,
                draft_is_created_by_me: true,
                in_process_by_user: current_principal(),
                last_changed_by_user: current_principal(),
            });
        }
    }

    pub fn key(&self) -> EntityKey {
        EntityKey {
            id: self.id(),
            is_active_entity: self.is_active_entity(),
        }
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

#[derive(Debug, Default, Clone)]
pub struct Navigation {
    pub single: bool,
    pub owning: bool,
    pub from_set: Option<String>,
    pub from_field: Option<String>,
    pub to_set: Option<String>,
    pub to_field: Option<String>,
}
impl Navigation {
    pub fn new(single: bool) -> Self {
        Self {
            single,
            ..Default::default()
        }
    }

    pub fn from(
        &mut self,
        set_name: impl Into<String>,
        field_name: impl Into<String>,
    ) -> &mut Self {
        self.from_set = Some(set_name.into());
        self.from_field = Some(field_name.into());
        self
    }

    pub fn to(&mut self, set_name: impl Into<String>, field_name: impl Into<String>) -> &mut Self {
        self.to_set = Some(set_name.into());
        self.to_field = Some(field_name.into());
        self
    }
}

/// Eine Schreiboperation fuer [`DataStore::write_entities`].
#[derive(Debug, Clone)]
pub enum WriteOp {
    Create(Value),
    Patch(EntityKey, Value),
    Delete(EntityKey),
}

#[async_trait::async_trait]
pub trait DataStore: Send + Sync {
    fn component_info(&self) -> String;
    // -- Collections --
    async fn get_collection(
        &self,
        set_name: &str,
        query: &ODataQuery,
    ) -> Result<Vec<DataRecord>, StoreError>;

    async fn count(&self, set_name: &str, query: &ODataQuery) -> usize;

    /// Liste und optional Gesamtzahl in einem Aufruf. Standard: getrennt ueber
    /// `get_collection` und `count`. Erweiterungen (Hooks) koennen die Zahl
    /// an ihre eigenen Aenderungen anpassen.
    async fn read_collection(
        &self,
        set_name: &str,
        query: &ODataQuery,
        with_count: bool,
    ) -> Result<(Vec<DataRecord>, Option<usize>), StoreError> {
        let records = self.get_collection(set_name, query).await?;
        let count = if with_count {
            Some(self.count(set_name, query).await)
        } else {
            None
        };
        Ok((records, count))
    }

    // -- Single Entity CRUD --
    async fn read_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        query: &ODataQuery,
    ) -> Result<DataRecord, StoreError>;

    async fn create_entity(&self, set_name: &str, data: &Value) -> Result<DataRecord, StoreError>;

    async fn patch_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        patch: &Value,
    ) -> Result<Value, StoreError>;

    async fn delete_entity(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError>;

    /// Delete an entity tree without also deleting the sibling (used for composition children).
    /// When deleting a child as part of discarding a draft parent, we don't want to also
    /// delete the active sibling.
    async fn delete_entity_tree_only(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<(), StoreError> {
        // Default implementation: just call delete_entity
        // Implementations should override to not delete sibling
        self.delete_entity(set_name, key).await
    }

    // -- Sibling Entity (draft <-> active) --
    async fn read_sibling_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<DataRecord, StoreError>;

    // -- Property --
    async fn get_property(
        &self,
        set_name: &str,
        key: &EntityKey,
        property: &str,
    ) -> Result<Value, StoreError>;

    // -- Persistence --
    async fn commit(&self);

    // -- Batch-Schreiben (Default: Schleife ueber die Einzelmethoden) --

    /// Fuehrt eine Liste von Schreiboperationen aus. Jede Operation liefert ihr
    /// eigenes Ergebnis, ein Fehler bricht die Liste nicht ab. Create und Patch
    /// liefern den vollstaendigen Satz (inklusive Hook-Aenderungen), Delete
    /// liefert `None`.
    async fn write_entities(
        &self,
        set_name: &str,
        ops: Vec<WriteOp>,
    ) -> Vec<Result<Option<DataRecord>, StoreError>> {
        let mut results = Vec::with_capacity(ops.len());
        for op in ops {
            let result = match op {
                WriteOp::Create(data) => self
                    .create_entity(set_name, &data)
                    .await
                    .map(Some),
                WriteOp::Patch(key, patch) => match self.patch_entity(set_name, &key, &patch).await
                {
                    Ok(_) => self
                        .read_entity(set_name, &key, &ODataQuery::new())
                        .await
                        .map(Some),
                    Err(e) => Err(e),
                },
                WriteOp::Delete(key) => self.delete_entity(set_name, &key).await.map(|()| None),
            };
            results.push(result);
        }
        results
    }

    // -- Default-Methoden (kein Implementierungszwang) --
    async fn update_entities(
        &self,
        set_name: &str,
        updates: &[(EntityKey, Value)],
    ) -> Result<(), StoreError> {
        for (key, patch) in updates {
            self.patch_entity(set_name, key, patch).await?;
        }
        Ok(())
    }

    async fn seed_records(&self, set_name: &str, records: Vec<Value>) {
        for record in records {
            let _ = self.create_entity(set_name, &record).await;
        }
    }

    fn navigation(&mut self, single: bool) -> &mut Navigation;

    /// Replace all registered navigations - used when the data dictionary is reloaded.
    fn set_navigations(&self, navigations: Vec<Navigation>);
}

/// Optionale Debug-Erweiterung. Bewusst getrennt vom Kern-Trait, damit
/// eine spaetere Postgres-Implementierung nicht gezwungen ist, einen
/// ungefilterten Full-Table-Scan anzubieten.
#[cfg(any(test, feature = "debug-tools"))]
#[async_trait::async_trait]
pub trait DataStoreDebugExt: DataStore {
    async fn get_records(&self, set_name: &str) -> Vec<Value>;
}

#[cfg(test)]
mod from_json_id_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn valid_uuid_id_is_kept() {
        let r = DataRecord::from_json(&json!({
            "ID": "00000000-0000-0000-0000-000000000042",
            "IsActiveEntity": true
        }))
        .unwrap();
        assert_eq!(r.id.to_string(), "00000000-0000-0000-0000-000000000042");
    }

    #[test]
    fn invalid_id_is_rejected_not_replaced() {
        let err = DataRecord::from_json(&json!({ "ID": "3963451", "IsActiveEntity": true }));
        assert!(matches!(err, Err(StoreError::Serialize(_))));
    }

    #[test]
    fn missing_id_gets_new_uuid() {
        let r = DataRecord::from_json(&json!({ "IsActiveEntity": false })).unwrap();
        assert!(!r.is_active_entity);
    }
}

#[cfg(test)]
mod principal_tests {
    use super::*;

    #[test]
    fn created_by_me_compares_with_the_request_user() {
        assert!(!draft_created_by_current_user("someone@example.org"));
        assert!(draft_created_by_current_user(FALLBACK_PRINCIPAL));
    }

    fn record() -> DataRecord {
        DataRecord::from_json(&json!({"name": "x"})).unwrap()
    }

    #[tokio::test]
    async fn draft_is_owned_by_the_request_user() {
        CURRENT_PRINCIPAL
            .scope(Some("peter@example.org".into()), async {
                let mut r = record();
                r.edit();
                let admin = r.draft_admin_data().unwrap();
                assert_eq!(admin.in_process_by_user, "peter@example.org");
                assert_eq!(admin.last_changed_by_user, "peter@example.org");
            })
            .await;
    }

    #[tokio::test]
    async fn draft_without_request_user_falls_back() {
        let mut r = record();
        r.edit();
        assert_eq!(r.draft_admin_data().unwrap().in_process_by_user, FALLBACK_PRINCIPAL);
    }
}
