//! SQLite-Implementierung des `DataStore`/`DraftDataStore`-Interfaces.
//!
//! Die Semantik ist bewusst identisch zum `YamlStore`: Datensaetze werden als
//! Dokumente (JSON) abgelegt, der Schluessel ist `(EntitySet, ID, IsActiveEntity)`.
//! Anstelle von `<root>/<Set>/{active,draft}/<uuid>.yaml` liegt jeder Datensatz
//! in einer Zeile der Tabelle `records`.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, RwLock};

use data_store::filter_eval::evaluate;
use data_store::{DataRecord, DataStore, DraftDataStore, EntityKey, Navigation, StoreError};
use odata::query::{Expr, ODataQuery, SortDirection};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use tracing::{debug, trace};
use uuid::Uuid;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS records (
    set_name  TEXT    NOT NULL,
    id        TEXT    NOT NULL,
    is_active INTEGER NOT NULL,
    payload   TEXT    NOT NULL,
    PRIMARY KEY (set_name, id, is_active)
);
CREATE INDEX IF NOT EXISTS records_by_set ON records (set_name, is_active);
";

pub struct SqliteStore {
    conn: Mutex<Connection>,
    navigations: RwLock<Vec<Navigation>>,
    current_change: RwLock<Option<Uuid>>,
}

fn sql_err(e: rusqlite::Error) -> StoreError {
    StoreError::Conversion(e.to_string())
}

impl SqliteStore {
    /// Oeffnet (bzw. erzeugt) `<root>/data.sqlite`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        let conn = Connection::open(root.join("data.sqlite")).map_err(sql_err)?;
        Self::from_connection(conn)
    }

    /// Fluechtiger Store - vor allem fuer Tests.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory().map_err(sql_err)?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> Result<Self, StoreError> {
        conn.execute_batch(SCHEMA).map_err(sql_err)?;
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "foreign_keys", "ON").ok();
        Ok(Self {
            conn: Mutex::new(conn),
            navigations: RwLock::new(vec![]),
            current_change: RwLock::new(None),
        })
    }

    pub fn get_change(&self) -> Uuid {
        let mut c = self.current_change.write().expect("current change lock");
        match *c {
            Some(x) => x,
            None => {
                let new_change = Uuid::new_v4();
                *c = Some(new_change);
                new_change
            }
        }
    }

    // -- Persistenz-Primitive -------------------------------------------------

    fn write_record(&self, set_name: &str, record: DataRecord) -> Result<(), StoreError> {
        let key = EntityKey::from(&record);
        let payload =
            serde_json::to_string(&record).map_err(|e| StoreError::Serialize(e.to_string()))?;
        self.conn
            .lock()
            .expect("connection lock")
            .execute(
                "INSERT INTO records (set_name, id, is_active, payload) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (set_name, id, is_active) DO UPDATE SET payload = excluded.payload",
                params![
                    set_name,
                    key.storage_id(),
                    key.is_active_entity as i64,
                    payload
                ],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    fn remove_record(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        self.conn
            .lock()
            .expect("connection lock")
            .execute(
                "DELETE FROM records WHERE set_name = ?1 AND id = ?2 AND is_active = ?3",
                params![set_name, key.storage_id(), key.is_active_entity as i64],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    fn load_entity_record(
        &self,
        key: &EntityKey,
        set_name: &str,
    ) -> Result<DataRecord, StoreError> {
        let payload: Option<String> = self
            .conn
            .lock()
            .expect("connection lock")
            .query_row(
                "SELECT payload FROM records WHERE set_name = ?1 AND id = ?2 AND is_active = ?3",
                params![set_name, key.storage_id(), key.is_active_entity as i64],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_err)?;
        match payload {
            Some(p) => parse_record(&p),
            None => Err(StoreError::not_found(format!(
                "{}/{}/{}",
                set_name,
                if key.is_active_entity {
                    "active"
                } else {
                    "draft"
                },
                key.storage_id()
            ))),
        }
    }

    /// Erst alle Drafts, dann alle aktiven Saetze, die noch keinen Draft haben.
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

    fn load_records_by_state(
        &self,
        set_name: &str,
        active: bool,
    ) -> Result<Vec<DataRecord>, StoreError> {
        let conn = self.conn.lock().expect("connection lock");
        let mut stmt = conn
            .prepare("SELECT payload FROM records WHERE set_name = ?1 AND is_active = ?2 ORDER BY id")
            .map_err(sql_err)?;
        let rows = stmt
            .query_map(params![set_name, active as i64], |row| {
                row.get::<_, String>(0)
            })
            .map_err(sql_err)?;
        let mut result = vec![];
        for row in rows {
            result.push(parse_record(&row.map_err(sql_err)?)?);
        }
        Ok(result)
    }

    /// Anzahl gespeicherter Saetze - Pendant zum Zaehlen der YAML-Dateien im
    /// jeweiligen `active`/`draft`-Verzeichnis.
    pub fn count_records(&self, set_name: &str, active: bool) -> usize {
        self.conn
            .lock()
            .expect("connection lock")
            .query_row(
                "SELECT COUNT(*) FROM records WHERE set_name = ?1 AND is_active = ?2",
                params![set_name, active as i64],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0) as usize
    }

    pub fn get_records(&self, set_name: &str) -> Vec<Value> {
        self.load_records(set_name)
            .unwrap_or_default()
            .iter()
            .map(|r| json!(r))
            .collect()
    }

    // -- Navigation / Kompositionen ------------------------------------------

    fn nav_hash_tab(
        &self,
        entity_set: &str,
    ) -> HashMap<(String, String), (bool, bool, String, String)> {
        let mut navs = HashMap::new();
        for nav in self
            .navigations
            .read()
            .expect("navigations lock")
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
                    navs.insert((from_set, from_field), (single, owning, to_set, to_field));
                } else if to_set == entity_set {
                    navs.insert((to_set, to_field), (!single, owning, from_set, from_field));
                }
            }
        }
        navs
    }

    fn extract_parent(
        &self,
        set_name: &str,
        rec: &DataRecord,
    ) -> Result<Option<(String, EntityKey)>, StoreError> {
        let mut result: Option<(String, EntityKey)> = None;
        for ((from_set, from_field), (collection, owned, to_set, _to_field)) in
            self.nav_hash_tab(set_name).iter()
        {
            if *collection && *owned {
                trace!("{from_set}.{from_field}");
                let field = rec.get(from_field).ok_or_else(|| {
                    StoreError::PropertyNotFound(format!("{from_set}.{from_field}"))
                })?;
                if let Some(r) = field.as_str() {
                    let refid =
                        Uuid::parse_str(r).map_err(|e| StoreError::Conflict(e.to_string()))?;
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
        key: &EntityKey,
    ) -> Result<Vec<(String, EntityKey)>, StoreError> {
        let mut result: Vec<(String, EntityKey)> = vec![];
        for ((_from_set, _from_field), (_collection, owned, to_set, to_field)) in
            self.nav_hash_tab(set_name).iter()
        {
            if *owned && to_set != "DraftAdministrativeData" {
                let query = ODataQuery::new().filter(Expr::equals_guid(key.id, to_field));
                result.extend(
                    self.get_collection(to_set, &query)?
                        .iter()
                        .map(|r| (to_set.clone(), EntityKey::from(r))),
                );
            }
        }
        Ok(result)
    }

    fn delete_entity_tree(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        self.remove_record(set_name, key)?;
        for (set, child) in self.get_children(set_name, key)?.iter() {
            self.delete_entity_tree(set, child)?;
        }
        Ok(())
    }

    fn draft_edit_child(&self, set_name: &str, active_key: &EntityKey) -> Result<(), StoreError> {
        if !active_key.is_active_entity {
            return Err(StoreError::InvalidState);
        }
        let mut draft_data = self.read_entity(set_name, active_key, &ODataQuery::new())?;
        draft_data.edit();
        self.write_record(set_name, draft_data)?;
        for (set, key) in self.get_children(set_name, active_key)?.iter() {
            self.draft_edit_child(set, key)?;
        }
        Ok(())
    }

    // -- $expand --------------------------------------------------------------

    fn expand_single_row(
        &self,
        from_id_field: &str,
        row: &mut DataRecord,
        single: bool,
        from_field: &str,
        to_entity: &str,
        to_field: &str,
    ) -> Result<(), StoreError> {
        if single {
            if let Some(from_id) = row.get(from_id_field) {
                match to_entity {
                    "DraftAdministrativeData" => {
                        if let Some(d) = row.draft_admin_data() {
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
                            _ => {
                                return Err(StoreError::Conflict(format!(
                                    "{to_entity}: mehr als ein Treffer fuer {from_field}"
                                )));
                            }
                        }
                    }
                }
            } else {
                row[from_field] = Value::Null;
            }
        } else {
            let q = ODataQuery::new().filter(Expr::equals_guid(row.id(), to_field));
            row[from_field] = json!(
                self.get_collection(to_entity, &q)?
                    .iter()
                    .map(|x| x.core_json())
                    .collect::<Vec<_>>()
            );
        }
        Ok(())
    }
}

fn parse_record(payload: &str) -> Result<DataRecord, StoreError> {
    serde_json::from_str(payload).map_err(|e| StoreError::Deserialize(e.to_string()))
}

fn compare_json(va: &Value, vb: &Value) -> Ordering {
    match (va, vb) {
        (Value::Number(x), Value::Number(y)) => x
            .as_f64()
            .partial_cmp(&y.as_f64())
            .unwrap_or(Ordering::Equal),
        (Value::String(x), Value::String(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

impl DataStore for SqliteStore {
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
            .into_iter()
            .map(|mut r| {
                for s in query.select.iter() {
                    r.touch(s);
                }
                r
            })
            .collect();

        for x in query.expand.iter() {
            let navs = self.navigations.read().expect("navigations lock").clone();
            for n in navs.iter() {
                if let Navigation {
                    single,
                    from_set: Some(from_entity),
                    from_field: Some(from_field),
                    to_set: Some(to_entity),
                    to_field: Some(to_field),
                    ..
                } = n
                {
                    if from_entity == set_name && from_field == &x.navigation_property {
                        let from_id_field = format!("{from_field}_ID");
                        for row in records.iter_mut() {
                            self.expand_single_row(
                                &from_id_field,
                                row,
                                *single,
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
        let mut q = query.clone();
        q.top = None;
        q.skip = None;
        q.expand.clear();
        q.select = vec![];
        self.get_collection(set_name, &q).map(|x| x.len()).unwrap_or(0)
    }

    fn read_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        query: &ODataQuery,
    ) -> Result<DataRecord, StoreError> {
        let navs = self.nav_hash_tab(set_name);
        let mut row = self.load_entity_record(key, set_name)?;
        for x in query.expand.iter() {
            let Some((single, _owning, to_set, to_field)) =
                navs.get(&(set_name.to_string(), x.navigation_property.clone()))
            else {
                return Err(StoreError::Metadata(format!(
                    "navigation declaration for {} missing.",
                    x.navigation_property
                )));
            };
            let from_id_field = format!("{}_ID", x.navigation_property);
            self.expand_single_row(
                &from_id_field,
                &mut row,
                *single,
                &x.navigation_property,
                to_set,
                to_field,
            )?;
        }
        Ok(row)
    }

    fn create_entity(&self, set_name: &str, data: &Value) -> Result<DataRecord, StoreError> {
        trace!("create entity {set_name}");
        let mut rec = DataRecord::from_json(data)?;
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

    fn patch_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        patch: &Value,
    ) -> Result<Value, StoreError> {
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

    fn delete_entity_tree_only(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        self.delete_entity_tree(set_name, key)
    }

    fn read_sibling_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<DataRecord, StoreError> {
        self.load_entity_record(&key.sibling(), set_name)
    }

    fn get_property(
        &self,
        set_name: &str,
        key: &EntityKey,
        property: &str,
    ) -> Result<Value, StoreError> {
        self.read_entity(set_name, key, &ODataQuery::new())?
            .get(property)
            .ok_or_else(|| StoreError::PropertyNotFound(property.into()))
    }

    fn commit(&self) {}
}

impl DraftDataStore for SqliteStore {
    fn draft_edit(&self, set_name: &str, active_key: &EntityKey) -> Result<Value, StoreError> {
        if !active_key.is_active_entity {
            return Err(StoreError::InvalidState);
        }
        let q = ODataQuery::new();
        let mut draft_data = self.read_entity(set_name, active_key, &q)?;

        // Ein Kind darf nur bearbeitet werden, wenn der Parent bereits im Draft ist.
        if let Some((parent_set, parent_key)) = self.extract_parent(set_name, &draft_data)? {
            if self
                .read_entity(&parent_set, &parent_key.sibling(), &q)
                .is_err()
            {
                return Err(StoreError::InvalidState);
            }
        }

        draft_data.edit();
        self.write_record(set_name, draft_data.clone())?;
        for (set, key) in self.get_children(set_name, active_key)?.iter() {
            self.draft_edit_child(set, key)?;
        }
        Ok(json!(draft_data))
    }

    fn draft_activate(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        debug!("activate draft {} {:?}", set_name, key);
        let mut draft = self.load_entity_record(key, set_name)?;
        for (set, child) in self.get_children(set_name, key)?.iter() {
            self.draft_activate(set, child)?;
        }
        draft.activate()?;
        self.write_record(set_name, draft.clone())?;
        self.remove_record(set_name, key)?;
        Ok(json!(draft))
    }

    fn draft_prepare(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        Ok(json!(self.load_entity_record(key, set_name)?))
    }
}
