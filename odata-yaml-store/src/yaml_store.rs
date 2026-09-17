use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use data_store::nav::{NavLink, composition_parent, nav_links};
use data_store::query::{apply_query, count_query};
use data_store::{DataRecord, DataStore, DraftDataStore, EntityKey, Navigation, StoreError};
use odata::query::{Expr, ODataQuery};
use serde_json::{Value, json};
use tracing::{debug, trace};
use uuid::Uuid;

const ACTIVE_DIR: &str = "active";
const DRAFT_DIR: &str = "draft";

pub struct YamlStore {
    root: PathBuf,
    current_change: RwLock<Option<Uuid>>,
    navigations: RwLock<Vec<Navigation>>,
}

impl YamlStore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        Ok(Self {
            root: root.into(),
            current_change: RwLock::new(None),
            navigations: RwLock::new(vec![]),
        })
    }

    pub fn get_change(&self) -> Uuid {
        let mut c = self.current_change.write().expect("current change lock");
        *c.get_or_insert_with(Uuid::new_v4)
    }

    // -- Ablage ---------------------------------------------------------------

    fn path_for_set(&self, set_name: &str, active: bool) -> PathBuf {
        let entity_path = self.root.join(set_name);
        if !entity_path.exists() {
            fs::create_dir_all(entity_path.join(ACTIVE_DIR)).expect("active dir");
            fs::create_dir_all(entity_path.join(DRAFT_DIR)).expect("draft dir");
        }
        entity_path.join(if active { ACTIVE_DIR } else { DRAFT_DIR })
    }

    fn path_for(&self, set_name: &str, key: &EntityKey) -> PathBuf {
        self.path_for_set(set_name, key.is_active_entity)
            .join(format!("{}.yaml", key.storage_id()))
    }

    fn write_record(&self, set_name: &str, record: DataRecord) -> Result<(), StoreError> {
        let path = self.path_for(set_name, &EntityKey::from(&record));
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

    fn load_entity_record(
        &self,
        key: &EntityKey,
        set_name: &str,
    ) -> Result<DataRecord, StoreError> {
        let path = self.path_for(set_name, key);
        if !path.exists() {
            return Err(StoreError::not_found(path));
        }
        load_record_from_file(&path)
    }

    /// Alle Datensaetze eines Zustands (`active` oder `draft`).
    fn load_records_by_state(
        &self,
        set_name: &str,
        active: bool,
    ) -> Result<Vec<DataRecord>, StoreError> {
        let mut result = vec![];
        for entry in fs::read_dir(self.path_for_set(set_name, active))? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
                continue;
            }
            result.push(load_record_from_file(&path)?);
        }
        result.sort_by_key(|r| r.id());
        Ok(result)
    }

    /// Erst alle Drafts, dann die aktiven Saetze, die noch keinen Draft haben.
    fn load_records(&self, set_name: &str) -> Result<Vec<DataRecord>, StoreError> {
        let mut result = self.load_records_by_state(set_name, false)?;
        for x in self.load_records_by_state(set_name, true)? {
            if result.iter().any(|r| r.id() == x.id()) {
                continue;
            }
            result.push(x);
        }
        Ok(result)
    }

    // -- Navigation / Kompositionen ------------------------------------------

    fn nav_links(&self, entity_set: &str) -> HashMap<String, NavLink> {
        nav_links(
            self.navigations.read().expect("navigations lock").as_slice(),
            entity_set,
        )
    }

    /// Der haltende Parent eines Kompositions-Kindes - `None` bei Wurzel-Sets.
    fn extract_parent(
        &self,
        set_name: &str,
        rec: &DataRecord,
    ) -> Result<Option<(String, EntityKey)>, StoreError> {
        let links = self.nav_links(set_name);
        let Some((field, link)) = composition_parent(&links) else {
            return Ok(None);
        };
        trace!("parent link {set_name}.{field}");
        let value = rec
            .get(field)
            .ok_or_else(|| StoreError::PropertyNotFound(format!("{set_name}.{field}")))?;
        let Some(raw) = value.as_str() else {
            return Ok(None);
        };
        let refid = Uuid::parse_str(raw).map_err(|e| StoreError::Conflict(e.to_string()))?;
        Ok(Some((
            link.to_set.clone(),
            EntityKey::new(refid, rec.is_active_entity()),
        )))
    }

    /// Kinder einer Komposition **im selben Zustand** wie der Parent.
    ///
    /// Bewusst nicht ueber `get_collection`: dort verdecken Drafts die aktiven
    /// Saetze, wodurch beim Aufraeumen eines aktiven Baums der aktive Kindsatz
    /// unsichtbar waere (bzw. faelschlich der Draft angefasst wuerde).
    fn children(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<Vec<(String, EntityKey)>, StoreError> {
        let mut result = vec![];
        for link in self.nav_links(set_name).values() {
            if !link.is_composition_child() {
                continue;
            }
            let candidates = self.load_records_by_state(&link.to_set, key.is_active_entity)?;
            result.extend(
                candidates
                    .iter()
                    .filter(|rec| references(rec, &link.to_field, key.id))
                    .map(|rec| (link.to_set.clone(), EntityKey::from(rec))),
            );
        }
        Ok(result)
    }

    fn delete_entity_tree(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        for (set, child) in self.children(set_name, key)?.iter() {
            self.delete_entity_tree(set, child)?;
        }
        self.remove_record(set_name, key)
    }

    fn draft_edit_tree(&self, set_name: &str, active_key: &EntityKey) -> Result<(), StoreError> {
        let mut draft = self.load_entity_record(active_key, set_name)?;
        draft.edit();
        self.write_record(set_name, draft)?;
        for (set, child) in self.children(set_name, active_key)?.iter() {
            self.draft_edit_tree(set, child)?;
        }
        Ok(())
    }

    // -- $expand --------------------------------------------------------------

    async fn expand_single_row(
        &self,
        row: &mut DataRecord,
        field: &str,
        link: &NavLink,
    ) -> Result<(), StoreError> {
        if !link.single {
            let q = ODataQuery::new().filter(Expr::equals_guid(row.id(), &link.to_field));
            row[field] = json!(
                self.get_collection(&link.to_set, &q).await?
                    .iter()
                    .map(|x| x.core_json())
                    .collect::<Vec<_>>()
            );
            return Ok(());
        }

        if link.to_set == data_store::DRAFT_ADMIN_SET {
            if let Some(d) = row.draft_admin_data() {
                row[field] = json!(d);
            }
            return Ok(());
        }

        let Some(from_id) = row.get(&format!("{field}_ID")) else {
            row[field] = Value::Null;
            return Ok(());
        };
        let q = ODataQuery::new().filter(Expr::equals_value(&from_id, "ID"));
        let subrows = self
            .get_collection(&link.to_set, &q).await?
            .iter()
            .map(|x| x.core_json())
            .collect::<Vec<_>>();
        row[field] = match subrows.as_slice() {
            [] => Value::Null,
            [v] => json!(v),
            _ => {
                return Err(StoreError::Conflict(format!(
                    "{}: mehr als ein Treffer fuer {field}",
                    link.to_set
                )));
            }
        };
        Ok(())
    }

    async fn expand_rows(
        &self,
        set_name: &str,
        rows: &mut [DataRecord],
        query: &ODataQuery,
        strict: bool,
    ) -> Result<(), StoreError> {
        if query.expand.is_empty() {
            return Ok(());
        }
        let links = self.nav_links(set_name);
        for item in query.expand.iter() {
            match links.get(&item.navigation_property) {
                Some(link) => {
                    for row in rows.iter_mut() {
                        self.expand_single_row(row, &item.navigation_property, link).await?;
                    }
                }
                None if strict => {
                    return Err(StoreError::Metadata(format!(
                        "navigation declaration for {} missing.",
                        item.navigation_property
                    )));
                }
                None => {}
            }
        }
        Ok(())
    }
}

fn references(rec: &DataRecord, field: &str, parent: Uuid) -> bool {
    rec.get(field)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .and_then(|s| Uuid::parse_str(&s).ok())
        == Some(parent)
}

fn load_record_from_file(path: &Path) -> Result<DataRecord, StoreError> {
    let content = fs::read_to_string(path)?;
    serde_yaml::from_str(&content)
        .map_err(|e| StoreError::Deserialize(format!("{}: {e}", path.display())))
}

#[async_trait::async_trait]
impl DataStore for YamlStore {
    fn navigation(&mut self, single: bool) -> &mut Navigation {
        let navigations = self.navigations.get_mut().expect("navigations lock");
        navigations.push(Navigation::new(single));
        navigations.last_mut().expect("last navigation")
    }

    fn set_navigations(&self, navigations: Vec<Navigation>) {
        *self.navigations.write().expect("navigations lock") = navigations;
    }

    async fn get_collection(
        &self,
        set_name: &str,
        query: &ODataQuery,
    ) -> Result<Vec<DataRecord>, StoreError> {
        debug!("get collection {set_name} {:?}", query);
        let mut records: Vec<DataRecord> = self
            .load_records(set_name)?
            .into_iter()
            .map(|mut r| {
                for s in query.select.iter() {
                    r.touch(s);
                }
                r
            })
            .collect();

        self.expand_rows(set_name, &mut records, query, false).await?;
        apply_query(&mut records, query);
        Ok(records)
    }

    async fn count(&self, set_name: &str, query: &ODataQuery) -> usize {
        self.get_collection(set_name, &count_query(query)).await
            .map(|x| x.len())
            .unwrap_or(0)
    }

    async fn read_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        query: &ODataQuery,
    ) -> Result<DataRecord, StoreError> {
        let mut row = [self.load_entity_record(key, set_name)?];
        self.expand_rows(set_name, &mut row, query, true).await?;
        let [row] = row;
        Ok(row)
    }

    async fn create_entity(&self, set_name: &str, data: &Value) -> Result<DataRecord, StoreError> {
        trace!("create entity {set_name}");
        let mut rec = DataRecord::from_json(data)?;
        // IsActiveEntity == true umgeht den Draft-Flow und schreibt direkt aktiv.
        match self.extract_parent(set_name, &rec)? {
            Some((parent_set, parent_key)) => {
                let parent = self.load_entity_record(&parent_key, &parent_set)?;
                match (rec.is_active_entity(), parent.draft_admin_data()) {
                    (true, None) => self.write_record(set_name, rec.clone())?,
                    (true, Some(_)) => return Err(StoreError::InvalidState),
                    (false, None) => return Err(StoreError::InvalidState),
                    (false, Some(parent_draft)) => {
                        rec.set_draft_uuid(parent_draft.draft_uuid);
                        self.write_record(set_name, rec.clone())?;
                    }
                }
            }
            None => {
                if !rec.is_active_entity() {
                    rec.create_draft();
                }
                self.write_record(set_name, rec.clone())?;
            }
        }
        Ok(rec)
    }

    async fn patch_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        patch: &Value,
    ) -> Result<Value, StoreError> {
        let mut r = self.load_entity_record(key, set_name)?;
        r.merge(patch);
        self.write_record(set_name, r)?;
        Ok(patch.clone())
    }

    async fn delete_entity(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        self.delete_entity_tree(set_name, key)?;
        if !key.is_active_entity {
            // Draft verwerfen heisst auch: den zugehoerigen aktiven Baum loeschen.
            self.delete_entity_tree(set_name, &key.sibling())?;
        }
        Ok(())
    }

    async fn delete_entity_tree_only(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        self.delete_entity_tree(set_name, key)
    }

    async fn read_sibling_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<DataRecord, StoreError> {
        self.load_entity_record(&key.sibling(), set_name)
    }

    async fn get_property(
        &self,
        set_name: &str,
        key: &EntityKey,
        property: &str,
    ) -> Result<Value, StoreError> {
        self.read_entity(set_name, key, &ODataQuery::new()).await?
            .get(property)
            .ok_or_else(|| StoreError::PropertyNotFound(property.into()))
    }

    async fn commit(&self) {}
}

#[async_trait::async_trait]
impl DraftDataStore for YamlStore {
    async fn draft_edit(&self, set_name: &str, active_key: &EntityKey) -> Result<Value, StoreError> {
        if !active_key.is_active_entity {
            return Err(StoreError::InvalidState);
        }
        let q = ODataQuery::new();
        let mut draft_data = self.read_entity(set_name, active_key, &q).await?;

        // Ein Kind darf nur bearbeitet werden, wenn der Parent schon im Draft ist.
        if let Some((parent_set, parent_key)) = self.extract_parent(set_name, &draft_data)? {
            if self
                .read_entity(&parent_set, &parent_key.sibling(), &q).await
                .is_err()
            {
                return Err(StoreError::InvalidState);
            }
        }

        draft_data.edit();
        self.write_record(set_name, draft_data.clone())?;
        for (set, child) in self.children(set_name, active_key)?.iter() {
            self.draft_edit_tree(set, child)?;
        }
        Ok(json!(draft_data))
    }

    async fn draft_activate(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        debug!("activate draft {} {:?}", set_name, key);
        if key.is_active_entity {
            return Err(StoreError::InvalidState);
        }
        let mut draft = self.load_entity_record(key, set_name)?;
        for (set, child) in self.children(set_name, key)?.iter() {
            self.draft_activate(set, child).await?;
        }
        draft.activate()?;
        self.write_record(set_name, draft.clone())?;
        self.remove_record(set_name, key)?;
        Ok(json!(draft))
    }

    async fn draft_prepare(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        Ok(json!(self.load_entity_record(key, set_name)?))
    }
}
