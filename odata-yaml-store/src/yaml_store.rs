use crate::datastore::{DataRecord, DataStore, Navigation};
use crate::error::StoreError;
use crate::filter_eval::evaluate;
use crate::key::EntityKey;
use crate::DraftDataStore;
use odata::query::{Expr, ODataQuery, SortDirection};
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;
use tracing::{info, trace};
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

        let q = ODataQuery::new();

        let mut draft_data = self.read_entity(set_name, active_key, &q)?;
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
        info!("get collection {set_name} {:?}", query);
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
        info!("total records: {}", records.len());
        for x in query.expand.iter() {
            for n in self.navigations.read().expect("navigations lock").iter() {
                if let Navigation {
                    single,
                    from_set: Some(from_entity),
                    from_field: Some(from_field),
                    to_set: Some(to_entity),
                    to_field: Some(to_field),
                } = n
                {
                    let from_id_field = format!("{from_field}_ID");
                    info!("... {:?}", n);
                    if from_entity == set_name && from_field == &x.navigation_property {
                        info!("using...");
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
        let p = self.path_for(set_name, key);
        let content = fs::read_to_string(&p)
            .or_else(|_| Err(StoreError::NotFound(format!("{p:?} for key {key:?}"))))?;
        let mut row = serde_yaml::from_str(&content)
            .or_else(|e| Err(StoreError::Deserialize(format!("{e}"))))
            .and_then(|x| DataRecord::from_json(&x))?;
        for x in query.expand.iter() {
            for n in self.navigations.read().expect("navigations lock").iter() {
                if let Navigation {
                    single,
                    from_set: Some(from_entity),
                    from_field: Some(from_field),
                    to_set: Some(to_entity),
                    to_field: Some(to_field),
                } = n
                {
                    let from_id_field = format!("{from_field}_ID");
                    info!("... {:?}", n);
                    if from_entity == set_name && from_field == &x.navigation_property {
                        info!("using...");
                        self.expand_single_row(
                            &from_id_field,
                            &mut row,
                            single,
                            from_field,
                            to_entity,
                            to_field,
                        )?;
                    }
                }
            }
        }
        Ok(row)
    }

    fn create_entity(&self, set_name: &str, data: &Value) -> Result<DataRecord, StoreError> {
        let mut rec: DataRecord = DataRecord::from_json(data)?;
        if !rec.is_active_entity() {
            rec.create_draft();
        }
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
        let mut r = self.read_entity(set_name, key, &ODataQuery::new())?;
        r.merge(patch);
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
        let entity = self.read_entity(set_name, key, &ODataQuery::new())?;
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
