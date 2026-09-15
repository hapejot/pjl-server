//! SQLite-Implementierung des `DataStore`/`DraftDataStore`-Interfaces.
//!
//! Die Semantik ist bewusst identisch zum `YamlStore`: Datensaetze werden als
//! Dokumente (JSON) abgelegt, der Schluessel ist `(EntitySet, ID, IsActiveEntity)`.
//! Anstelle von `<root>/<Set>/{active,draft}/<uuid>.yaml` liegt jeder Datensatz
//! in einer Zeile der Tabelle `records`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, RwLock};

use data_store::nav::{composition_parent, nav_links, NavLink};
use data_store::query::{apply_query, count_query};
use data_store::{DataRecord, DataStore, DraftDataStore, EntityKey, Navigation, StoreError};
use odata::query::{Expr, ODataQuery};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
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
    StoreError::Conversion(format!("sqlite: {e}"))
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
        Self::from_connection(Connection::open_in_memory().map_err(sql_err)?)
    }

    fn from_connection(conn: Connection) -> Result<Self, StoreError> {
        conn.execute_batch(SCHEMA).map_err(sql_err)?;
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        Ok(Self {
            conn: Mutex::new(conn),
            navigations: RwLock::new(vec![]),
            current_change: RwLock::new(None),
        })
    }

    pub fn get_change(&self) -> Uuid {
        let mut c = self.current_change.write().expect("current change lock");
        *c.get_or_insert_with(Uuid::new_v4)
    }

    // -- Ablage ---------------------------------------------------------------

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
                "{set_name}/{}/{}",
                if key.is_active_entity {
                    "active"
                } else {
                    "draft"
                },
                key.storage_id()
            ))),
        }
    }

    /// Alle Datensaetze eines Zustands (`active` oder `draft`).
    fn load_records_by_state(
        &self,
        set_name: &str,
        active: bool,
    ) -> Result<Vec<DataRecord>, StoreError> {
        let conn = self.conn.lock().expect("connection lock");
        let mut stmt = conn
            .prepare(
                "SELECT payload FROM records WHERE set_name = ?1 AND is_active = ?2 ORDER BY id",
            )
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

    fn nav_links(&self, entity_set: &str) -> HashMap<String, NavLink> {
        nav_links(
            self.navigations
                .read()
                .expect("navigations lock")
                .as_slice(),
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

    fn expand_single_row(
        &self,
        row: &mut DataRecord,
        field: &str,
        link: &NavLink,
    ) -> Result<(), StoreError> {
        if !link.single {
            let q = ODataQuery::new().filter(Expr::equals_guid(row.id(), &link.to_field));
            row[field] = json!(self
                .get_collection(&link.to_set, &q)?
                .iter()
                .map(|x| x.core_json())
                .collect::<Vec<_>>());
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
            .get_collection(&link.to_set, &q)?
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

    fn expand_rows(
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
                        self.expand_single_row(row, &item.navigation_property, link)?;
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

fn parse_record(payload: &str) -> Result<DataRecord, StoreError> {
    serde_json::from_str(payload).map_err(|e| StoreError::Deserialize(e.to_string()))
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

        self.expand_rows(set_name, &mut records, query, false)?;
        apply_query(&mut records, query);
        Ok(records)
    }

    fn count(&self, set_name: &str, query: &ODataQuery) -> usize {
        self.get_collection(set_name, &count_query(query))
            .map(|x| x.len())
            .unwrap_or(0)
    }

    fn read_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        query: &ODataQuery,
    ) -> Result<DataRecord, StoreError> {
        let mut row = [self.load_entity_record(key, set_name)?];
        self.expand_rows(set_name, &mut row, query, true)?;
        let [row] = row;
        Ok(row)
    }

    fn create_entity(&self, set_name: &str, data: &Value) -> Result<DataRecord, StoreError> {
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

    fn patch_entity(
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

    fn delete_entity(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        self.delete_entity_tree(set_name, key)?;
        if !key.is_active_entity {
            // Draft verwerfen heisst auch: den zugehoerigen aktiven Baum loeschen.
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

        // Ein Kind darf nur bearbeitet werden, wenn der Parent schon im Draft ist.
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
        for (set, child) in self.children(set_name, active_key)?.iter() {
            self.draft_edit_tree(set, child)?;
        }
        Ok(json!(draft_data))
    }

    fn draft_activate(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        debug!("activate draft {} {:?}", set_name, key);
        if key.is_active_entity {
            return Err(StoreError::InvalidState);
        }
        let mut draft = self.load_entity_record(key, set_name)?;
        for (set, child) in self.children(set_name, key)?.iter() {
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
