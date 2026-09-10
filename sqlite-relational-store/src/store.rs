//! Relationaler SQLite-Store: eine Tabelle je Entity-Set.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use data_store::schema::{
    Association, Cardinality, EntitySet, Schema, SchemaSource, ID_COLUMN, IS_ACTIVE_COLUMN,
};
use data_store::{DataRecord, DataStore, DraftDataStore, EntityKey, Navigation, StoreError};
use odata::query::ODataQuery;
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, params_from_iter};
use serde_json::{Map, Value, json};
use tracing::{debug, trace};
use uuid::Uuid;

use crate::ddl::{
    self, DRAFT_ADMIN_COLUMN, DRAFT_ADMIN_TABLE, DRAFT_MESSAGES_COLUMN, quote, sql_err,
};
use crate::sql::{Translator, order_by_clause, paging_clause, placeholders};
use crate::value::{json_to_sql, sql_to_json};

const ALIAS: &str = "t";

pub struct RelationalStore {
    conn: Mutex<Connection>,
    schema_source: Arc<dyn SchemaSource>,
    schema: Mutex<Arc<Schema>>,
    path: Option<PathBuf>,
}

impl RelationalStore {
    /// Oeffnet `<root>/data.sqlite` und bringt die Datenbank auf den Stand des
    /// Schemas.
    pub fn open(
        root: impl Into<PathBuf>,
        schema_source: Arc<dyn SchemaSource>,
    ) -> Result<Self, StoreError> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        let path = root.join("data.sqlite");
        let conn = Connection::open(&path).map_err(sql_err)?;
        Self::from_connection(conn, schema_source, Some(path))
    }

    pub fn open_in_memory(schema_source: Arc<dyn SchemaSource>) -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory().map_err(sql_err)?;
        Self::from_connection(conn, schema_source, None)
    }

    fn from_connection(
        conn: Connection,
        schema_source: Arc<dyn SchemaSource>,
        path: Option<PathBuf>,
    ) -> Result<Self, StoreError> {
        // Ohne das hier sind alle Kaskaden wirkungslos.
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(sql_err)?;
        conn.pragma_update(None, "journal_mode", "WAL").ok();

        let schema = schema_source
            .schema()
            .map_err(|e| StoreError::Metadata(e.to_string()))?;
        ddl::ensure_schema(&conn, &schema)?;
        ddl::set_applied_revision(&conn, schema_source.revision().0)?;

        Ok(Self {
            conn: Mutex::new(conn),
            schema_source,
            schema: Mutex::new(schema),
            path,
        })
    }

    /// Uebernimmt einen neuen Schemastand und migriert die Datenbank.
    pub fn refresh_schema(&self) -> Result<bool, StoreError> {
        let revision = self.schema_source.revision();
        let conn = self.conn.lock().expect("connection lock");
        if ddl::applied_revision(&conn) == Some(revision.0) {
            return Ok(false);
        }
        let schema = self
            .schema_source
            .schema()
            .map_err(|e| StoreError::Metadata(e.to_string()))?;
        ddl::ensure_schema(&conn, &schema)?;
        ddl::set_applied_revision(&conn, revision.0)?;
        *self.schema.lock().expect("schema lock") = schema;
        Ok(true)
    }

    /// Spalten, die in der Datenbank stehen, aber nicht mehr im Schema.
    pub fn pending_destructive_changes(&self) -> Result<Vec<String>, StoreError> {
        let conn = self.conn.lock().expect("connection lock");
        ddl::pending_destructive_changes(&conn, &self.schema())
    }

    pub fn schema(&self) -> Arc<Schema> {
        self.schema.lock().expect("schema lock").clone()
    }

    pub fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    fn entity_set(&self, set_name: &str) -> Result<Arc<Schema>, StoreError> {
        let schema = self.schema();
        schema
            .set(set_name)
            .map_err(|e| StoreError::Metadata(e.to_string()))?;
        Ok(schema)
    }

    /// Anzahl gespeicherter Saetze je Aktivierungszustand.
    pub fn count_state(&self, set_name: &str, is_active: bool) -> usize {
        let Ok(schema) = self.entity_set(set_name) else {
            return 0;
        };
        let set = schema.set(set_name).expect("geprueft");
        let sql = if set.draft_enabled {
            format!(
                "SELECT COUNT(*) FROM {} WHERE {} = ?1",
                quote(set_name),
                quote(IS_ACTIVE_COLUMN)
            )
        } else if is_active {
            format!("SELECT COUNT(*) FROM {}", quote(set_name))
        } else {
            return 0;
        };
        let conn = self.conn.lock().expect("connection lock");
        let result: rusqlite::Result<i64> = if set.draft_enabled {
            conn.query_row(&sql, [i64::from(is_active)], |r| r.get(0))
        } else {
            conn.query_row(&sql, [], |r| r.get(0))
        };
        result.unwrap_or(0) as usize
    }

    // -- SELECT-Bausteine ----------------------------------------------------

    /// Projektionsliste inklusive der abgeleiteten Draft-Flags.
    ///
    /// `HasActiveEntity`/`HasDraftEntity` werden **berechnet**, nicht
    /// gespeichert: im relationalen Modell sind sie reine Ableitungen aus der
    /// Existenz des Zwillings und koennen so nicht veralten.
    fn projection(&self, set: &EntitySet) -> String {
        let mut parts: Vec<String> = set
            .columns()
            .map(|c| format!("{ALIAS}.{}", quote(&c.name)))
            .collect();

        if set.draft_enabled {
            let table = quote(&set.name);
            parts.push(format!(
                "({ALIAS}.{is_active} = 0 AND EXISTS (SELECT 1 FROM {table} s \
                 WHERE s.{id} = {ALIAS}.{id} AND s.{is_active} = 1)) AS {has_active}",
                is_active = quote(IS_ACTIVE_COLUMN),
                id = quote(ID_COLUMN),
                has_active = quote("HasActiveEntity"),
            ));
            parts.push(format!(
                "({ALIAS}.{is_active} = 1 AND EXISTS (SELECT 1 FROM {table} s \
                 WHERE s.{id} = {ALIAS}.{id} AND s.{is_active} = 0)) AS {has_draft}",
                is_active = quote(IS_ACTIVE_COLUMN),
                id = quote(ID_COLUMN),
                has_draft = quote("HasDraftEntity"),
            ));
            parts.push(format!("{ALIAS}.{}", quote(DRAFT_ADMIN_COLUMN)));
            parts.push(format!("{ALIAS}.{}", quote(DRAFT_MESSAGES_COLUMN)));
        }
        parts.join(", ")
    }

    /// Drafts verdecken ihren aktiven Zwilling.
    fn shadow_predicate(set: &EntitySet) -> Option<String> {
        if !set.draft_enabled {
            return None;
        }
        Some(format!(
            "({ALIAS}.{is_active} = 0 OR NOT EXISTS (SELECT 1 FROM {table} d \
             WHERE d.{id} = {ALIAS}.{id} AND d.{is_active} = 0))",
            table = quote(&set.name),
            id = quote(ID_COLUMN),
            is_active = quote(IS_ACTIVE_COLUMN),
        ))
    }

    fn load_draft_admin(&self, draft_uuid: &str) -> Result<Option<Value>, StoreError> {
        let conn = self.conn.lock().expect("connection lock");
        let mut stmt = conn
            .prepare(&format!(
                "SELECT \"DraftUUID\", \"DraftIsCreatedByMe\", \"InProcessByUser\", \
                 \"LastChangedByUser\" FROM {} WHERE \"DraftUUID\" = ?1",
                quote(DRAFT_ADMIN_TABLE)
            ))
            .map_err(sql_err)?;
        let mut rows = stmt.query([draft_uuid]).map_err(sql_err)?;
        let Some(row) = rows.next().map_err(sql_err)? else {
            return Ok(None);
        };
        Ok(Some(json!({
            "DraftUUID": row.get::<_, String>(0).map_err(sql_err)?,
            "DraftIsCreatedByMe": row.get::<_, i64>(1).map_err(sql_err)? != 0,
            "InProcessByUser": row.get::<_, Option<String>>(2).map_err(sql_err)?.unwrap_or_default(),
            "LastChangedByUser": row.get::<_, Option<String>>(3).map_err(sql_err)?.unwrap_or_default(),
        })))
    }

    fn store_draft_admin(&self, record: &DataRecord) -> Result<Option<String>, StoreError> {
        let Some(admin) = record.draft_admin_data() else {
            return Ok(None);
        };
        let uuid = admin.draft_uuid.to_string();
        let conn = self.conn.lock().expect("connection lock");
        conn.execute(
            &format!(
                "INSERT INTO {} (\"DraftUUID\", \"DraftIsCreatedByMe\", \"InProcessByUser\", \
                 \"LastChangedByUser\") VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT (\"DraftUUID\") DO UPDATE SET \
                 \"DraftIsCreatedByMe\" = excluded.\"DraftIsCreatedByMe\", \
                 \"InProcessByUser\" = excluded.\"InProcessByUser\", \
                 \"LastChangedByUser\" = excluded.\"LastChangedByUser\"",
                quote(DRAFT_ADMIN_TABLE)
            ),
            rusqlite::params![
                uuid,
                i64::from(admin.draft_is_created_by_me),
                admin.in_process_by_user,
                admin.last_changed_by_user,
            ],
        )
        .map_err(sql_err)?;
        Ok(Some(uuid))
    }

    /// Schreibt einen Datensatz (INSERT ... ON CONFLICT DO UPDATE).
    fn write_record(&self, set: &EntitySet, record: &DataRecord) -> Result<(), StoreError> {
        let admin_id = self.store_draft_admin(record)?;

        let mut columns: Vec<String> = vec![];
        let mut values: Vec<SqlValue> = vec![];
        for column in set.columns() {
            let value = match column.name.as_str() {
                ID_COLUMN => json!(record.id()),
                IS_ACTIVE_COLUMN => json!(record.is_active_entity()),
                other => record.get(other).unwrap_or(Value::Null),
            };
            columns.push(column.name.clone());
            values.push(json_to_sql(&value, &column.data_type, &column.name)?);
        }
        if set.draft_enabled {
            columns.push(DRAFT_ADMIN_COLUMN.to_string());
            values.push(match admin_id {
                Some(id) => SqlValue::Text(id),
                None => SqlValue::Null,
            });
            columns.push(DRAFT_MESSAGES_COLUMN.to_string());
            values.push(if record.draft_messages().is_empty() {
                SqlValue::Null
            } else {
                SqlValue::Text(json!(record.draft_messages()).to_string())
            });
        }

        let placeholders_list = placeholders(columns.len(), 0);
        let assignments: Vec<String> = columns
            .iter()
            .filter(|c| c.as_str() != ID_COLUMN && c.as_str() != IS_ACTIVE_COLUMN)
            .map(|c| format!("{0} = excluded.{0}", quote(c)))
            .collect();
        let key_columns: Vec<String> = set.key_columns().iter().map(|c| quote(&c.name)).collect();

        let sql = format!(
            "INSERT INTO {table} ({cols}) VALUES ({vals}) ON CONFLICT ({keys}) DO UPDATE SET {sets}",
            table = quote(&set.name),
            cols = columns.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", "),
            vals = placeholders_list,
            keys = key_columns.join(", "),
            sets = if assignments.is_empty() {
                format!("{0} = {0}", quote(ID_COLUMN))
            } else {
                assignments.join(", ")
            },
        );
        trace!("write: {sql}");
        self.conn
            .lock()
            .expect("connection lock")
            .execute(&sql, params_from_iter(values))
            .map_err(sql_err)?;
        Ok(())
    }

    fn key_predicate(set: &EntitySet, alias: &str) -> String {
        if set.draft_enabled {
            format!(
                "{alias}.{} = ?1 AND {alias}.{} = ?2",
                quote(ID_COLUMN),
                quote(IS_ACTIVE_COLUMN)
            )
        } else {
            format!("{alias}.{} = ?1", quote(ID_COLUMN))
        }
    }

    fn key_params(set: &EntitySet, key: &EntityKey) -> Vec<SqlValue> {
        let mut params = vec![SqlValue::Text(key.storage_id())];
        if set.draft_enabled {
            params.push(SqlValue::Integer(i64::from(key.is_active_entity)));
        }
        params
    }

    /// Einzelsatz ueber den Primaerschluessel.
    fn load_one(&self, set: &EntitySet, key: &EntityKey) -> Result<DataRecord, StoreError> {
        let where_clause = Self::key_predicate(set, ALIAS);
        let records = self.query_records(
            set,
            Some(where_clause),
            Self::key_params(set, key),
            None,
            "",
        )?;
        records.into_iter().next().ok_or_else(|| {
            StoreError::not_found(format!(
                "{}/{}/{}",
                set.name,
                if key.is_active_entity {
                    "active"
                } else {
                    "draft"
                },
                key.storage_id()
            ))
        })
    }

    fn finish_record(
        &self,
        mut map: Map<String, Value>,
        admin_id: Option<String>,
        messages: Option<String>,
    ) -> Result<DataRecord, StoreError> {
        if let Some(admin_id) = admin_id {
            if let Some(admin) = self.load_draft_admin(&admin_id)? {
                map.insert("DraftAdministrativeData".into(), admin);
            }
        }
        if let Some(messages) = messages {
            if let Ok(parsed) = serde_json::from_str::<Value>(&messages) {
                map.insert("DraftMessages".into(), parsed);
            }
        }
        DataRecord::from_json(&Value::Object(map))
    }

    /// Fuehrt ein SELECT aus und materialisiert die Zeilen.
    fn query_records(
        &self,
        set: &EntitySet,
        where_clause: Option<String>,
        params: Vec<SqlValue>,
        order_by: Option<String>,
        paging: &str,
    ) -> Result<Vec<DataRecord>, StoreError> {
        let mut sql = format!(
            "SELECT {} FROM {} {ALIAS}",
            self.projection(set),
            quote(&set.name)
        );
        if let Some(where_clause) = where_clause {
            sql.push_str(&format!(" WHERE {where_clause}"));
        }
        if let Some(order_by) = order_by {
            sql.push_str(&format!(" ORDER BY {order_by}"));
        }
        sql.push_str(paging);
        debug!("query: {sql}");

        // Rohdaten einsammeln, damit die Verbindung fuer die Nachladeschritte
        // (Draft-Admin) wieder frei ist.
        let mut raw: Vec<(Map<String, Value>, Option<String>, Option<String>)> = vec![];
        {
            let conn = self.conn.lock().expect("connection lock");
            let mut stmt = conn.prepare(&sql).map_err(sql_err)?;
            let mut rows = stmt.query(params_from_iter(params)).map_err(sql_err)?;
            while let Some(row) = rows.next().map_err(sql_err)? {
                let mut map = Map::new();
                for (index, column) in set.columns().enumerate() {
                    let value = row.get_ref(index).map_err(sql_err)?;
                    map.insert(column.name.clone(), sql_to_json(value, &column.data_type));
                }
                let mut admin_id = None;
                let mut messages = None;
                if set.draft_enabled {
                    let base = set.columns().count();
                    map.insert(
                        "HasActiveEntity".into(),
                        json!(row.get::<_, i64>(base).map_err(sql_err)? != 0),
                    );
                    map.insert(
                        "HasDraftEntity".into(),
                        json!(row.get::<_, i64>(base + 1).map_err(sql_err)? != 0),
                    );
                    admin_id = row.get(base + 2).map_err(sql_err)?;
                    messages = row.get(base + 3).map_err(sql_err)?;
                }
                raw.push((map, admin_id, messages));
            }
        }

        raw.into_iter()
            .map(|(map, admin, messages)| self.finish_record(map, admin, messages))
            .collect()
    }

    // -- Kompositionen --------------------------------------------------------

    /// Schluessel der Kompositionskinder **im selben Aktivierungszustand**
    /// wie der Parent.
    fn child_keys(
        &self,
        set: &EntitySet,
        key: &EntityKey,
    ) -> Result<Vec<(String, EntityKey)>, StoreError> {
        let schema = self.schema();
        let mut result = vec![];
        for assoc in set.composition_children() {
            let child = schema
                .set(&assoc.target_set)
                .map_err(|e| StoreError::Metadata(e.to_string()))?;
            let fk = assoc.foreign_key.column();
            let mut sql = format!(
                "SELECT {} FROM {} WHERE {} = ?1",
                quote(ID_COLUMN),
                quote(&child.name),
                quote(fk)
            );
            let mut params = vec![SqlValue::Text(key.storage_id())];
            if child.draft_enabled {
                sql.push_str(&format!(" AND {} = ?2", quote(IS_ACTIVE_COLUMN)));
                params.push(SqlValue::Integer(i64::from(key.is_active_entity)));
            }
            let conn = self.conn.lock().expect("connection lock");
            let mut stmt = conn.prepare(&sql).map_err(sql_err)?;
            let ids = stmt
                .query_map(params_from_iter(params), |row| row.get::<_, String>(0))
                .map_err(sql_err)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(sql_err)?;
            for id in ids {
                let uuid = Uuid::parse_str(&id).map_err(|e| StoreError::Conflict(e.to_string()))?;
                result.push((
                    child.name.clone(),
                    EntityKey::new(uuid, key.is_active_entity),
                ));
            }
        }
        Ok(result)
    }

    fn delete_row(&self, set: &EntitySet, key: &EntityKey) -> Result<(), StoreError> {
        // ON DELETE CASCADE raeumt die Kompositionskinder ab - im Gegensatz
        // zum Dokumentenstore gibt es hier keine Rekursion in Rust.
        let sql = format!(
            "DELETE FROM {} WHERE {} = ?1{}",
            quote(&set.name),
            quote(ID_COLUMN),
            if set.draft_enabled {
                format!(" AND {} = ?2", quote(IS_ACTIVE_COLUMN))
            } else {
                String::new()
            }
        );
        self.conn
            .lock()
            .expect("connection lock")
            .execute(&sql, params_from_iter(Self::key_params(set, key)))
            .map_err(sql_err)?;
        Ok(())
    }

    fn draft_edit_tree(&self, set: &EntitySet, key: &EntityKey) -> Result<(), StoreError> {
        let mut record = self.load_one(set, key)?;
        record.edit();
        self.write_record(set, &record)?;
        let schema = self.schema();
        for (child_set, child_key) in self.child_keys(set, key)? {
            let child = schema
                .set(&child_set)
                .map_err(|e| StoreError::Metadata(e.to_string()))?;
            self.draft_edit_tree(child, &child_key)?;
        }
        Ok(())
    }

    // -- $expand --------------------------------------------------------------

    fn expand_rows(
        &self,
        set: &EntitySet,
        rows: &mut [DataRecord],
        query: &ODataQuery,
        strict: bool,
    ) -> Result<(), StoreError> {
        if query.expand.is_empty() || rows.is_empty() {
            return Ok(());
        }
        for item in query.expand.iter() {
            let name = &item.navigation_property;
            if name == data_store::DRAFT_ADMIN_SET {
                for row in rows.iter_mut() {
                    if let Some(admin) = row.draft_admin_data() {
                        row[name] = json!(admin);
                    }
                }
                continue;
            }
            match set.get_association(name) {
                Ok(assoc) => self.expand_association(rows, assoc)?,
                Err(e) if strict => return Err(StoreError::Metadata(e.to_string())),
                Err(_) => {}
            }
        }
        Ok(())
    }

    fn expand_association(
        &self,
        rows: &mut [DataRecord],
        assoc: &Association,
    ) -> Result<(), StoreError> {
        let schema = self.schema();
        let target = schema
            .set(&assoc.target_set)
            .map_err(|e| StoreError::Metadata(e.to_string()))?;

        match assoc.cardinality {
            Cardinality::ToOne => {
                // Ein Batch-SELECT ueber alle Fremdschluessel statt N Einzelabfragen.
                let column = assoc.foreign_key.column();
                let ids: Vec<String> = rows
                    .iter()
                    .filter_map(|r| r.get(column))
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect();
                if ids.is_empty() {
                    for row in rows.iter_mut() {
                        row[&assoc.name] = Value::Null;
                    }
                    return Ok(());
                }
                let mut where_clause = format!(
                    "{ALIAS}.{} IN ({})",
                    quote(ID_COLUMN),
                    placeholders(ids.len(), 0)
                );
                if let Some(shadow) = Self::shadow_predicate(target) {
                    where_clause = format!("{where_clause} AND {shadow}");
                }
                let params: Vec<SqlValue> = ids.iter().cloned().map(SqlValue::Text).collect();
                let found = self.query_records(target, Some(where_clause), params, None, "")?;
                let by_id: HashMap<String, Value> = found
                    .iter()
                    .map(|r| (r.id().to_string(), r.core_json()))
                    .collect();
                for row in rows.iter_mut() {
                    let value = row
                        .get(column)
                        .and_then(|v| v.as_str().map(str::to_string))
                        .and_then(|id| by_id.get(&id).cloned())
                        .unwrap_or(Value::Null);
                    row[&assoc.name] = value;
                }
            }
            Cardinality::ToMany => {
                let fk = assoc.foreign_key.column();
                let ids: Vec<String> = rows.iter().map(|r| r.id().to_string()).collect();
                let mut where_clause = format!(
                    "{ALIAS}.{} IN ({})",
                    quote(fk),
                    placeholders(ids.len(), 0)
                );
                if let Some(shadow) = Self::shadow_predicate(target) {
                    where_clause = format!("{where_clause} AND {shadow}");
                }
                let params: Vec<SqlValue> = ids.iter().cloned().map(SqlValue::Text).collect();
                let found = self.query_records(target, Some(where_clause), params, None, "")?;

                let mut grouped: HashMap<String, Vec<Value>> = HashMap::new();
                for record in found.iter() {
                    if let Some(parent) = record.get(fk).and_then(|v| v.as_str().map(str::to_string))
                    {
                        grouped.entry(parent).or_default().push(record.core_json());
                    }
                }
                for row in rows.iter_mut() {
                    let children = grouped.remove(&row.id().to_string()).unwrap_or_default();
                    row[&assoc.name] = Value::Array(children);
                }
            }
        }
        Ok(())
    }
}

impl DataStore for RelationalStore {
    /// Metadaten kommen ueber [`SchemaSource`], nicht ueber den Store-Trait.
    fn navigation(&mut self, _single: bool) -> &mut Navigation {
        panic!(
            "RelationalStore bezieht seine Metadaten aus einer SchemaSource - \
             DataStore::navigation() wird nicht unterstuetzt"
        )
    }

    fn set_navigations(&self, _navigations: Vec<Navigation>) {
        // Bewusst wirkungslos: das Schema ist die Quelle der Wahrheit.
        debug!("set_navigations() ignoriert - RelationalStore nutzt die SchemaSource");
    }

    fn get_collection(
        &self,
        set_name: &str,
        query: &ODataQuery,
    ) -> Result<Vec<DataRecord>, StoreError> {
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");

        let mut clauses: Vec<String> = Self::shadow_predicate(set).into_iter().collect();
        let mut params: Vec<SqlValue> = vec![];
        if let Some(filter) = &query.filter {
            let sql = Translator::new(set, ALIAS).filter(filter)?;
            // Filterparameter kommen nach den (parameterlosen) Shadow-Klauseln.
            clauses.push(sql.text);
            params.extend(sql.params);
        }
        let where_clause = if clauses.is_empty() {
            None
        } else {
            Some(clauses.join(" AND "))
        };

        let mut records = self.query_records(
            set,
            where_clause,
            params,
            order_by_clause(set, ALIAS, query)?,
            &paging_clause(query),
        )?;

        for record in records.iter_mut() {
            for s in query.select.iter() {
                record.touch(s);
            }
        }
        self.expand_rows(set, &mut records, query, false)?;
        Ok(records)
    }

    fn count(&self, set_name: &str, query: &ODataQuery) -> usize {
        let mut q = data_store::query::count_query(query);
        q.orderby.clear();
        self.get_collection(set_name, &q)
            .map(|r| r.len())
            .unwrap_or(0)
    }

    fn read_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        query: &ODataQuery,
    ) -> Result<DataRecord, StoreError> {
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        let mut rows = [self.load_one(set, key)?];
        self.expand_rows(set, &mut rows, query, true)?;
        let [row] = rows;
        Ok(row)
    }

    fn create_entity(&self, set_name: &str, data: &Value) -> Result<DataRecord, StoreError> {
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        let mut record = DataRecord::from_json(data)?;

        // Unbekannte Felder sind ein Fehler: im relationalen Modell gibt es
        // keine Spalte, in der sie verschwinden koennten.
        if let Some(object) = data.as_object() {
            for field in object.keys() {
                let known = set.has_column(field)
                    || matches!(
                        field.as_str(),
                        "ID"
                            | "IsActiveEntity"
                            | "HasActiveEntity"
                            | "HasDraftEntity"
                            | "DraftAdministrativeData"
                            | "DraftMessages"
                    )
                    || set.get_association(field).is_ok();
                if !known {
                    return Err(StoreError::PropertyNotFound(format!(
                        "{set_name}.{field}"
                    )));
                }
            }
        }

        if let Some(parent) = set.composition_parent() {
            let parent_set = schema
                .set(&parent.target_set)
                .map_err(|e| StoreError::Metadata(e.to_string()))?;
            let fk = parent.foreign_key.column();
            let parent_id = record
                .get(fk)
                .and_then(|v| v.as_str().map(str::to_string))
                .ok_or_else(|| StoreError::PropertyNotFound(format!("{set_name}.{fk}")))?;
            let parent_uuid =
                Uuid::parse_str(&parent_id).map_err(|e| StoreError::Conflict(e.to_string()))?;
            let parent_key = EntityKey::new(parent_uuid, record.is_active_entity());
            let parent_record = self.load_one(parent_set, &parent_key)?;

            match (record.is_active_entity(), parent_record.draft_admin_data()) {
                (true, None) => {}
                (true, Some(_)) => return Err(StoreError::InvalidState),
                (false, None) => return Err(StoreError::InvalidState),
                (false, Some(admin)) => record.set_draft_uuid(admin.draft_uuid),
            }
        } else if !record.is_active_entity() {
            record.create_draft();
        }

        self.write_record(set, &record)?;
        Ok(record)
    }

    fn patch_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        patch: &Value,
    ) -> Result<Value, StoreError> {
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        if let Some(object) = patch.as_object() {
            for field in object.keys() {
                if !set.has_column(field) {
                    return Err(StoreError::PropertyNotFound(format!("{set_name}.{field}")));
                }
                let column = set
                    .get_column(field)
                    .map_err(|e| StoreError::Metadata(e.to_string()))?;
                if column.immutable {
                    return Err(StoreError::Conflict(format!(
                        "{set_name}.{field} ist unveraenderlich"
                    )));
                }
            }
        }
        let mut record = self.load_one(set, key)?;
        record.merge(patch);
        self.write_record(set, &record)?;
        Ok(patch.clone())
    }

    fn delete_entity(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        self.delete_row(set, key)?;
        if !key.is_active_entity {
            self.delete_row(set, &key.sibling())?;
        }
        Ok(())
    }

    fn delete_entity_tree_only(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        self.delete_row(set, key)
    }

    fn read_sibling_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<DataRecord, StoreError> {
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        self.load_one(set, &key.sibling())
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

impl DraftDataStore for RelationalStore {
    fn draft_edit(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        if !key.is_active_entity {
            return Err(StoreError::InvalidState);
        }
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        let mut record = self.load_one(set, key)?;

        // Kinder duerfen nur mit dem Parent in den Draft.
        if let Some(parent) = set.composition_parent() {
            let parent_set = schema
                .set(&parent.target_set)
                .map_err(|e| StoreError::Metadata(e.to_string()))?;
            let fk = parent.foreign_key.column();
            let parent_id = record
                .get(fk)
                .and_then(|v| v.as_str().map(str::to_string))
                .ok_or_else(|| StoreError::PropertyNotFound(format!("{set_name}.{fk}")))?;
            let parent_uuid =
                Uuid::parse_str(&parent_id).map_err(|e| StoreError::Conflict(e.to_string()))?;
            if self
                .load_one(parent_set, &EntityKey::new(parent_uuid, false))
                .is_err()
            {
                return Err(StoreError::InvalidState);
            }
        }

        record.edit();
        self.write_record(set, &record)?;
        for (child_set, child_key) in self.child_keys(set, key)? {
            let child = schema
                .set(&child_set)
                .map_err(|e| StoreError::Metadata(e.to_string()))?;
            self.draft_edit_tree(child, &child_key)?;
        }
        Ok(json!(record))
    }

    fn draft_activate(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        if key.is_active_entity {
            return Err(StoreError::InvalidState);
        }
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        let mut record = self.load_one(set, key)?;

        // Erst die Kinder: der aktive Parent existiert bereits, der aktive
        // Kindsatz braucht ihn fuer den Fremdschluessel.
        for (child_set, child_key) in self.child_keys(set, key)? {
            self.draft_activate(&child_set, &child_key)?;
        }

        record.activate()?;
        self.write_record(set, &record)?;
        self.delete_row(set, key)?;
        Ok(json!(record))
    }

    fn draft_prepare(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        Ok(json!(self.load_one(set, key)?))
    }
}
