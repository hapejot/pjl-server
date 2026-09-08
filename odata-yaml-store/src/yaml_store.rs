use crate::datastore::{DataRecord, DataStore, Navigation};
use crate::error::StoreError;
use crate::filter_eval::evaluate;
use crate::key::EntityKey;
use crate::DraftDataStore;
use odata::query::{Expr, ODataQuery, SortDirection};
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;
use tracing::{debug, trace};
use uuid::Uuid;

pub struct YamlStore {
    root: PathBuf,
    current_change: RwLock<Option<Uuid>>,
    navigations: RwLock<Vec<Navigation>>,
    // cache: RwLock<HashMap<String, HashMap<Uuid, DataRecord>>>,
}

impl DraftDataStore for YamlStore {
    fn draft_edit(&self, set_name: &str, active_key: &EntityKey) -> Result<Value, StoreError> {
        // let draft_key = active_key.sibling(); // is_active_entity = false
        assert!(active_key.is_active_entity);
        if !active_key.is_active_entity {
            return Err(StoreError::InvalidState);
        }

        let q = ODataQuery::new();

        let mut draft_data = self.read_entity(set_name, active_key, &q)?;
        let parent = self.extract_parent(set_name, &draft_data)?;
        if let Some((parent_set, parent_key)) = parent {
            assert!(parent_key.is_active_entity);
            if let Err(_) = self.read_entity(&parent_set, &parent_key.sibling(), &q) {
                // Zum Parent konnte kein Draft gelesen werden, also Fehler
                return Err(StoreError::InvalidState);
            }
        }
        draft_data.edit();
        self.write_record(set_name, draft_data.clone())?;
        for (set, key) in self.get_children(set_name, active_key)?.iter() {
            self.draft_edit_child(&draft_data, set, key)?;
        }

        Ok(json!(draft_data))
    }

    fn draft_activate(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        debug!("activate draft {} {:?}", set_name, key);
        let path = self.path_for(set_name, key);
        let mut draft = load_record_from_file(path)?;
        for (set, key) in self.get_children(set_name, key)?.iter() {
            self.draft_activate(set, key)?;
        }

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
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        let store = Self {
            root,
            current_change: RwLock::new(None),
            navigations: RwLock::new(vec![]),
            // cache: RwLock::new(HashMap::new()),
        };
        // store.load_all()?;
        Ok(store)
    }

    fn draft_edit_child(
        &self,
        draft_data: &DataRecord,
        set_name: &str,
        active_key: &EntityKey,
    ) -> Result<(), StoreError> {
        //
        // let draft_key = active_key.sibling(); // is_active_entity = false
        let _ = draft_data;
        assert!(active_key.is_active_entity);
        if !active_key.is_active_entity {
            return Err(StoreError::InvalidState);
        }

        let q = ODataQuery::new();

        let mut draft_data = self.read_entity(set_name, active_key, &q)?;
        draft_data.edit();
        self.write_record(set_name, draft_data.clone())?;
        for (set, key) in self.get_children(set_name, active_key)?.iter() {
            self.draft_edit_child(&draft_data, set, key)?;
        }

        Ok(())
    }
    fn extract_parent(
        &self,
        set_name: &str,
        rec: &DataRecord,
    ) -> Result<Option<(String, EntityKey)>, StoreError> {
        let mut result: Option<(String, EntityKey)> = None;
        let nav = self.nav_hash_tab(set_name);
        for ((from_set, from_field), (collection, owned, to_set, _to_field)) in nav.iter() {
            if *collection && *owned {
                trace!("{from_set}.{from_field}");
                if let Some(r) = rec
                    .get(from_field)
                    .expect(&format!(
                        "expected field {} in record {:#?}",
                        from_field, rec
                    ))
                    .as_str()
                {
                    let refid =
                        Uuid::parse_str(r).map_err(|e| StoreError::Conflict(e.to_string()))?;
                    trace!("refid: {refid}");
                    result = Some((to_set.into(), EntityKey::new(refid, rec.is_active_entity())));
                }
                break;
            }
        }
        Ok(result)
    }

    fn get_children(
        &self,
        set_name: &str,
        active_key: &EntityKey,
    ) -> Result<Vec<(String, EntityKey)>, StoreError> {
        let mut result: Vec<(String, EntityKey)> = vec![];
        let nav = self.nav_hash_tab(set_name);
        let guid = active_key.id;
        for ((_from_set, _from_field), (_collection, owned, to_set, _to_field)) in nav.iter() {
            if *owned {
                let query = ODataQuery::new().filter(Expr::equals_guid(guid, _to_field));
                match to_set.as_str() {
                    "DraftAdministrativeData" => {}
                    _ => result.extend(
                        self.get_collection(to_set, &query)?
                            .iter()
                            .map(|r| (to_set.into(), r.into())),
                    ),
                }
            }
        }
        Ok(result)
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
        const DRAFT: &str = "draft";
        const ACTIVE: &str = "active";
        let state = match draft {
            true => DRAFT,
            false => ACTIVE,
        };
        let entity_path = self.root.join(set_name);
        if !entity_path.exists() {
            fs::create_dir_all(&entity_path).expect("entity path");
            fs::create_dir(&entity_path.join(ACTIVE)).expect("active");
            fs::create_dir(&entity_path.join(DRAFT)).expect("draft");
        }
        entity_path.join(state)
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

    fn expand_single_row(
        &self,
        from_id_field: &String,
        row: &mut DataRecord,
        single: &bool,
        from_field: &String,
        to_entity: &String,
        to_field: &String,
    ) -> Result<(), StoreError> {
        Ok(if *single {
            if let Some(from_id) = row.get(from_id_field.as_str()) {
                match to_entity.as_str() {
                    "DraftAdministrativeData" => {
                        if let Some(d) = row.draft_admin_data() {
                            debug!("copy admin data {:?} to {}", &row, from_field);
                            row[from_field] = json!(d);
                        }
                    }
                    _ => {
                        let q = ODataQuery::new().filter(Expr::equals_value(&from_id, "ID"));
                        let subrows = self
                            .get_collection(to_entity, &q)?
                            .iter()
                            .map(|x| x.core_json())
                            .collect::<Vec<_>>();
                        match subrows.as_slice() {
                            [] => row[from_field] = Value::Null,
                            [v] => row[from_field] = json!(v),
                            _ => todo!(),
                        }
                    }
                }
            } else {
                row[from_field] = Value::Null
            }
        } else {
            let q = ODataQuery::new().filter(Expr::equals_guid(row.id(), to_field));
            let subrows = json!(self
                .get_collection(to_entity, &q)?
                .iter()
                .map(|x| x.core_json())
                .collect::<Vec<_>>());
            row[from_field] = subrows;
        })
    }

    fn nav_hash_tab(
        &self,
        entity_set: &str,
    ) -> HashMap<(String, String), (bool, bool, String, String)> {
        let mut navs = HashMap::new();
        for nav in self
            .navigations
            .read()
            .expect("navigations")
            .iter()
            .cloned()
        {
            if let Navigation {
                single,
                owning,
                from_set: Some(from_set),
                from_field: Some(from_field),
                to_set: Some(to_set),
                to_field: Some(to_field),
            } = nav
            {
                if from_set == entity_set {
                    debug!(
                        "Navigation: {}.{} {}->{} {}.{}",
                        &from_set, &from_field, &single, &owning, &to_set, &to_field
                    );
                    navs.insert((from_set, from_field), (single, owning, to_set, to_field));
                } else if to_set == entity_set {
                    debug!(
                        "Navigation: {}.{} {}<-{} {}.{}",
                        &from_set, &from_field, &single, &owning, &to_set, &to_field
                    );
                    navs.insert((to_set, to_field), (!single, owning, from_set, from_field));
                }
            }
        }
        navs
    }

    fn load_entity_record(
        &self,
        key: &EntityKey,
        set_name: &str,
    ) -> Result<DataRecord, StoreError> {
        let p = self.path_for(set_name, key);
        let content = fs::read_to_string(&p).or_else(|_| Err(StoreError::not_found(p)))?;
        let row = serde_yaml::from_str(&content)?;
        // .or_else(|e| Err(StoreError::Deserialize(format!("{e}"))))
        // .and_then(|x| DataRecord::from_json(&x))?;
        Ok(row)
    }
    fn delete_entity_tree(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        self.remove_record(set_name, key)?;
        for (set, key) in self.get_children(set_name, key)?.iter() {
            self.delete_entity_tree(set, key)?;
        }
        Ok(())
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
    let content =
        fs::read_to_string(&path).expect(&format!("load record from file {}", path.display()));
    let record: DataRecord = serde_yaml::from_str(&content)
        .map_err(|e| StoreError::Deserialize(format!("{path:?}: {e}")))?;
    Ok(record)
}

impl DataStore for YamlStore {
    fn navigation(&mut self, single: bool) -> &mut Navigation {
        let navigations = self.navigations.get_mut().expect("navigations lock");
        navigations.push(Navigation::new(single));
        navigations.last_mut().expect("last navigation")
    }

    fn set_navigations(&self, navigations: Vec<Navigation>) {
        *self.navigations.write().expect("navigations lock") = navigations;
    }

    fn get_collection(
        &self,
        set_name: &str,
        query: &ODataQuery,
    ) -> Result<Vec<DataRecord>, StoreError> {
        debug!("get collection {set_name} {:?}", query);
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
        debug!("total records: {}", records.len());
        for x in query.expand.iter() {
            for n in self.navigations.read().expect("navigations lock").iter() {
                if let Navigation {
                    single,
                    owning,
                    from_set: Some(from_entity),
                    from_field: Some(from_field),
                    to_set: Some(to_entity),
                    to_field: Some(to_field),
                } = n
                {
                    let _ = owning;
                    let from_id_field = format!("{from_field}_ID");
                    debug!("... {:?}", n);
                    if from_entity == set_name && from_field == &x.navigation_property {
                        debug!("using...");
                        for row in records.iter_mut() {
                            self.expand_single_row(
                                &from_id_field,
                                row,
                                single,
                                from_field,
                                to_entity,
                                to_field,
                            )?;
                        }
                    }
                }
            }
        }

        if let Some(filter) = &query.filter {
            records.retain(|r| evaluate(r, filter) == Value::Bool(true));
        }

        for item in query.orderby.iter().rev() {
            records.sort_by(|a, b| {
                let (va, vb) = (
                    evaluate(a, &Expr::Identifier(item.property.clone())),
                    evaluate(b, &Expr::Identifier(item.property.clone())),
                );
                let ord = compare_json(&va, &vb);
                if item.direction == SortDirection::Desc {
                    ord.reverse()
                } else {
                    ord
                }
            });
        }

        if let Some(skip) = query.skip {
            records.drain(..(skip as usize).min(records.len()));
        }
        if let Some(top) = query.top {
            records.truncate(top as usize);
        }
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

    fn read_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        query: &ODataQuery,
    ) -> Result<DataRecord, StoreError> {
        let navs = self.nav_hash_tab(set_name);
        let set_name = set_name.to_string();

        let mut row = self.load_entity_record(key, &set_name)?;
        for x in query.expand.iter() {
            if let Some((single, owning, to_set, to_field)) =
                navs.get(&(set_name.clone(), x.navigation_property.clone()))
            {
                let _ = owning;
                let from_id_field = format!("{}_ID", x.navigation_property);
                self.expand_single_row(
                    &from_id_field,
                    &mut row,
                    single,
                    &x.navigation_property,
                    to_set,
                    to_field,
                )?;
            } else {
                return Err(StoreError::Metadata(format!(
                    "navigation declaration for {} missing.",
                    x.navigation_property
                )));
            }
        }
        Ok(row)
    }

    fn create_entity(&self, set_name: &str, data: &Value) -> Result<DataRecord, StoreError> {
        trace!("create entity {set_name}");
        let mut rec: DataRecord = DataRecord::from_json(data)?;
        // IsActiveEntity
        // true - der Draft-Flow wird umgangen und es wird direkt als active gespeichert.
        // false - regulärer Draft-Flow und es wird zunächst
        let parent = self.extract_parent(set_name, &rec)?;
        match &parent {
            Some((parent_set, parent_key)) => {
                let parent = self.load_entity_record(parent_key, parent_set)?;
                match (rec.is_active_entity(), parent.draft_admin_data()) {
                    (true, None) => {
                        self.write_record(set_name, rec.clone())?;
                    }
                    (true, Some(_)) => return Err(StoreError::InvalidState),
                    (false, None) => return Err(StoreError::InvalidState),
                    (false, Some(parent_draft)) => {
                        rec.set_draft_uuid(parent_draft.draft_uuid);
                        self.write_record(set_name, rec.clone())?;
                    }
                }
            }
            None => {
                if rec.is_active_entity() {
                    self.write_record(set_name, rec.clone())?;
                } else {
                    rec.create_draft();
                    self.write_record(set_name, rec.clone())?;
                }
            }
        }
        Ok(rec)
    }

    fn patch_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        patch: &Value,
    ) -> Result<Value, StoreError> {
        trace!("patching yaml");
        let mut r = self.read_entity(set_name, key, &ODataQuery::new())?;
        r.merge(patch);
        self.write_record(set_name, r)?;
        Ok(patch.clone())
    }

    fn delete_entity(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        self.delete_entity_tree(set_name, key)?;
        if !key.is_active_entity {
            self.delete_entity_tree(set_name, &key.sibling())?;
        }
        Ok(())
    }

    fn get_property(
        &self,
        set_name: &str,
        key: &EntityKey,
        property: &str,
    ) -> Result<Value, StoreError> {
        let entity = self.read_entity(set_name, key, &ODataQuery::new())?;
        entity
            .get(property)
            .ok_or_else(|| StoreError::PropertyNotFound(property.into()))
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

fn compare_json(va: &Value, vb: &Value) -> Ordering {
    match (va, vb) {
        (Value::Number(x), Value::Number(y)) => x
            .as_f64()
            .partial_cmp(&y.as_f64())
            .unwrap_or(Ordering::Equal),
        (Value::String(x), Value::String(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => todo!("Comparison not implemented for these JSON value types"),
    }
}
