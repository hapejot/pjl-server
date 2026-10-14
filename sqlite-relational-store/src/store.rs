//! Relationaler SQLite-Store: eine Tabelle je Entity-Set.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use data_store::schema::{
    Association, Cardinality, EntitySet, Schema, SchemaSource, ID_COLUMN, IS_ACTIVE_COLUMN,
};
use data_store::{
    json_to_param, param_to_json, DataRecord, DataStore, DraftDataStore, EntityKey, Navigation,
    SqlParam, StoreError,
};
use odata::query::{Expr, ODataQuery};
use rusqlite::backup::Backup;
use rusqlite::{params_from_iter, Connection};
use serde_json::{json, Map, Value};
use std::backtrace::Backtrace;
use tracing::*;
use uuid::Uuid;

use crate::catalog::SqliteCatalog;
use crate::value::{from_sqlite, to_sqlite};
use relational_core::ddl::{
    self, quote, DRAFT_ADMIN_COLUMN, DRAFT_ADMIN_TABLE, DRAFT_MESSAGES_COLUMN,
};
use relational_core::dialect::{Dialect, SQLITE};
use relational_core::sql::query::{order_by_clause, Translator};
use relational_core::sql::{paging_clause, placeholders};

/// Fuehrt `body` in einer Transaktion aus.
///
/// Ein abgebrochener Schreibvorgang darf keinen halben Zustand hinterlassen:
/// ein Draft-Kopf ohne Verwaltungsdaten, ein aktivierter Parent ohne seine
/// Kinder, ein geloeschter Draft ohne den zugehoerigen aktiven Satz. Ohne
/// Klammer war das bisher nur deshalb unauffaellig, weil der Mutex auf der
/// Verbindung die Zugriffe faktisch serialisiert hat.
macro_rules! in_transaction {
    ($self:expr, |$conn:ident| $body:block) => {{
        debug!("begin transaction");
        let guard = $self.conn.lock().expect("connection lock");
        let $conn = guard
            .unchecked_transaction()
            .map_err(|e| sql_err!(Conversion, e))?;
        debug!("transaction: process body");
        let result = (|| $body)()?;
        $conn.commit().map_err(|e| sql_err!(Conversion, e))?;
        debug!("end transaction {:?}", result);
        result
    }};
}

macro_rules! sql_err {
    ($variant:ident, $e:expr) => {{
        let bt = Backtrace::capture();
        StoreError::$variant(format!(
            "SQL ({}:{}): {} in {bt}",
            module_path!(),
            line!(),
            $e
        ))
    }};
}

/// Setzt die von [`ddl::migration_plan`] geplanten Anweisungen ab.
///
/// Die Planung kennt keine Verbindung; hier wird sie ausgefuehrt.
async fn apply_schema(
    conn: &Mutex<Connection>,
    dialect: &dyn Dialect,
    schema: &Schema,
) -> Result<(), StoreError> {
    let plan = ddl::migration_plan(&SqliteCatalog::new(conn), dialect, schema).await?;
    let conn = conn.lock().expect("connection lock");
    for statement in plan {
        debug!("DDL: {statement}");
        conn.execute_batch(&statement)
            .map_err(|e| sql_err!(Conversion, e))?;
    }
    Ok(())
}

/// Liest die zuletzt angewandte Revision.
fn applied_revision(conn: &Connection) -> Option<u64> {
    conn.query_row(&ddl::revision_select_sql(), [], |row| row.get::<_, i64>(0))
        .ok()
        .map(|v| v as u64)
}

fn set_applied_revision(
    conn: &Connection,
    dialect: &dyn Dialect,
    revision: u64,
) -> Result<(), StoreError> {
    conn.execute(&ddl::revision_delete_sql(), [])
        .map_err(|e| sql_err!(Conversion, e))?;
    conn.execute(&ddl::revision_insert_sql(dialect), [revision as i64])
        .map_err(|e| sql_err!(Conversion, e))?;
    Ok(())
}

pub struct RelationalStore {
    conn: Mutex<Connection>,
    schema_source: Arc<dyn SchemaSource>,
    schema: Mutex<Arc<Schema>>,
    path: Option<PathBuf>,
}

impl RelationalStore {
    /// Oeffnet `<root>/data.sqlite` und bringt die Datenbank auf den Stand des
    /// Schemas.
    pub async fn open(
        root: impl Into<PathBuf>,
        schema_source: Arc<dyn SchemaSource>,
    ) -> Result<Self, StoreError> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        let path = root.join("data.sqlite");
        // let conn = Connection::open(&path).map_err(|e| sql_err!(Metadata, e))?;
        let mut conn = Connection::open_in_memory().map_err(|e| sql_err!(Metadata, e))?;
        info!("created in-memory database");
        if path.exists() {
            let disk = Connection::open(&path).map_err(|e| sql_err!(Metadata, e))?;
            let backup = Backup::new(&disk, &mut conn).map_err(|e| sql_err!(Metadata, e))?;
            backup
                .run_to_completion(256, Duration::ZERO, None)
                .map_err(|e| sql_err!(Metadata, e))?;
            info!("loaded backup from {} into memory", path.display());
        }
        Self::from_connection(conn, schema_source, Some(path)).await
    }

    pub async fn open_in_memory(schema_source: Arc<dyn SchemaSource>) -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory().map_err(|e| sql_err!(Metadata, e))?;
        Self::from_connection(conn, schema_source, None).await
    }

    async fn from_connection(
        conn: Connection,
        schema_source: Arc<dyn SchemaSource>,
        path: Option<PathBuf>,
    ) -> Result<Self, StoreError> {
        // Ohne das hier sind alle Kaskaden wirkungslos.
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| sql_err!(Metadata, e))?;
        conn.pragma_update(None, "journal_mode", "WAL").ok();

        let schema = schema_source
            .schema()
            .map_err(|e| StoreError::Metadata(e.to_string()))?;
        let conn = Mutex::new(conn);
        apply_schema(&conn, &SQLITE, &schema).await?;
        set_applied_revision(
            &conn.lock().expect("connection lock"),
            &SQLITE,
            schema_source.revision().0,
        )?;

        Ok(Self {
            conn,
            schema_source,
            schema: Mutex::new(schema),
            path,
        })
    }

    /// Uebernimmt einen neuen Schemastand und migriert die Datenbank.
    pub async fn refresh_schema(&self) -> Result<bool, StoreError> {
        let revision = self.schema_source.revision();
        if applied_revision(&self.conn.lock().expect("connection lock")) == Some(revision.0) {
            return Ok(false);
        }
        let schema = self
            .schema_source
            .schema()
            .map_err(|e| StoreError::Metadata(e.to_string()))?;
        apply_schema(&self.conn, &SQLITE, &schema).await?;
        set_applied_revision(
            &self.conn.lock().expect("connection lock"),
            &SQLITE,
            revision.0,
        )?;
        *self.schema.lock().expect("schema lock") = schema;
        Ok(true)
    }

    /// Spalten, die in der Datenbank stehen, aber nicht mehr im Schema.
    pub async fn pending_destructive_changes(&self) -> Result<Vec<String>, StoreError> {
        ddl::pending_destructive_changes(&SqliteCatalog::new(&self.conn), &SQLITE, &self.schema())
            .await
    }

    /// Der SQL-Dialekt dieses Stores.
    fn dialect(&self) -> &'static dyn Dialect {
        &SQLITE
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

    /// Anzahl der Draft-Verwaltungssaetze.
    ///
    /// Diagnose- und Testhilfe: die Tabelle haengt an keinem Entity-Set und
    /// ist ueber den `DataStore`-Trait nicht sichtbar. Ein Satz ohne
    /// zugehoerigen Draft-Kopf ist verwaist - genau das soll nicht
    /// vorkommen.
    pub fn count_draft_admin(&self) -> usize {
        let conn = self.conn.lock().expect("connection lock");
        conn.query_row(
            &format!("SELECT COUNT(*) FROM {}", quote(DRAFT_ADMIN_TABLE)),
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0) as usize
    }

    // -- SELECT-Bausteine ----------------------------------------------------

    fn load_draft_admin(
        &self,
        conn: &Connection,
        draft_uuid: &str,
    ) -> Result<Option<Value>, StoreError> {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT \"DraftUUID\", \"DraftIsCreatedByMe\", \"InProcessByUser\", \
                 \"LastChangedByUser\" FROM {} WHERE \"DraftUUID\" = ?1",
                quote(DRAFT_ADMIN_TABLE)
            ))
            .map_err(|e| sql_err!(Metadata, e))?;
        let mut rows = stmt
            .query([draft_uuid])
            .map_err(|e| sql_err!(Metadata, e))?;
        let Some(row) = rows.next().map_err(|e| sql_err!(Metadata, e))? else {
            return Ok(None);
        };
        Ok(Some(json!({
            "DraftUUID": row.get::<_, String>("DraftUUID").map_err(|e| sql_err!(Metadata,e))?,
            "DraftIsCreatedByMe": row.get::<_, i64>("DraftIsCreatedByMe").map_err(|e| sql_err!(Metadata,e))? != 0,
            "InProcessByUser": row.get::<_, Option<String>>("InProcessByUser").map_err(|e| sql_err!(Metadata,e))?.unwrap_or_default(),
            "LastChangedByUser": row.get::<_, Option<String>>("LastChangedByUser").map_err(|e| sql_err!(Metadata,e))?.unwrap_or_default(),
        })))
    }

    fn store_draft_admin(
        &self,
        conn: &Connection,
        record: &DataRecord,
    ) -> Result<Option<String>, StoreError> {
        let Some(admin) = record.draft_admin_data() else {
            return Ok(None);
        };
        let uuid = admin.draft_uuid.to_string();
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
        .map_err(|e| sql_err!(Metadata, e))?;
        Ok(Some(uuid))
    }

    /// Schreibt einen Datensatz (INSERT ... ON CONFLICT DO UPDATE).
    fn write_record(
        &self,
        conn: &Connection,
        set: &EntitySet,
        record: &DataRecord,
    ) -> Result<(), StoreError> {
        // Laeuft in derselben Transaktion wie der Satz selbst: ein
        // Draft-Kopf ohne Verwaltungsdaten waere nicht mehr aktivierbar.
        let admin_id = self.store_draft_admin(conn, record)?;

        let mut columns: Vec<String> = vec![];
        let mut values: Vec<SqlParam> = vec![];
        for column in set.columns() {
            let value = match column.name.as_str() {
                ID_COLUMN => json!(record.id()),
                IS_ACTIVE_COLUMN => json!(record.is_active_entity()),
                other => record.get(other).unwrap_or(Value::Null),
            };
            columns.push(column.name.clone());
            values.push(json_to_param(&value, &column.data_type, &column.name)?);
        }
        if set.draft_enabled {
            columns.push(DRAFT_ADMIN_COLUMN.to_string());
            values.push(match admin_id {
                Some(id) => SqlParam::Text(id),
                None => SqlParam::Null,
            });
            columns.push(DRAFT_MESSAGES_COLUMN.to_string());
            values.push(if record.draft_messages().is_empty() {
                SqlParam::Null
            } else {
                SqlParam::Text(json!(record.draft_messages()).to_string())
            });
        }

        let placeholders_list = placeholders(self.dialect(), columns.len(), 0);
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
        trace!(".....: {values:?}");
        conn.execute(&sql, params_from_iter(values.iter().map(to_sqlite)))
            .map_err(|e| sql_err!(Metadata, e))?;
        Ok(())
    }

    fn key_params(set: &EntitySet, key: &EntityKey) -> Vec<SqlParam> {
        let mut params = vec![SqlParam::Text(key.storage_id())];
        if set.draft_enabled {
            params.push(SqlParam::Bool(key.is_active_entity));
        }
        params
    }

    /// Einzelsatz ueber den Primaerschluessel.
    fn load_one(
        &self,
        conn: &Connection,
        set: &EntitySet,
        key: &EntityKey,
    ) -> Result<DataRecord, StoreError> {
        let mut t = Translator::new(set, self.dialect())
            .filter(&Expr::equals_guid(key.id.to_string(), "ID"));
        if set.draft_enabled {
            t = t.filter(&Expr::equals_value(
                &json!(key.is_active_entity),
                "IsActiveEntity",
            ));
        }
        let q = t.build()?;
        let records = self.query_records(conn, set, q.text, q.params, None, "")?;
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
        conn: &Connection,
        mut map: Map<String, Value>,
        admin_id: Option<String>,
        messages: Option<String>,
    ) -> Result<DataRecord, StoreError> {
        if let Some(admin_id) = admin_id {
            if let Some(admin) = self.load_draft_admin(conn, &admin_id)? {
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
        conn: &Connection,
        set: &EntitySet,
        query_text: String,
        params: Vec<SqlParam>,
        order_by: Option<String>,
        paging: &str,
    ) -> Result<Vec<DataRecord>, StoreError> {
        let mut sql = query_text;
        if let Some(order_by) = order_by {
            sql.push_str(&format!(" ORDER BY {order_by}"));
        }
        sql.push_str(paging);
        debug!("query: {sql}");

        // Rohdaten einsammeln, bevor nachgeladen wird: das Statement haelt
        // eine Ausleihe auf die Verbindung, die Nachladeschritte brauchen sie
        // selbst.
        let mut raw: Vec<(Map<String, Value>, Option<String>, Option<String>)> = vec![];
        {
            let mut stmt = conn.prepare(&sql).map_err(|e| sql_err!(Metadata, e))?;
            // Nur die tatsaechlich projizierten Spalten materialisieren.
            // Grund: $select verkleinert die SELECT-Liste. Wir muessen also
            // den Result-Set als Wahrheit nehmen und pro Spalte den Datentyp
            // aus dem Schema nachschlagen (fuer sql_to_json). Die Zuordnung
            // erfolgt namensbasiert -- robust gegen jede Umordnung.
            let projected: Vec<(String, Option<data_store::schema::DataType>)> = stmt
                .column_names()
                .into_iter()
                .map(|n| {
                    let name = n.to_string();
                    let data_type = set
                        .columns()
                        .find(|c| c.name == name)
                        .map(|c| c.data_type.clone());
                    (name, data_type)
                })
                .collect();
            let mut rows = stmt
                .query(params_from_iter(params.iter().map(to_sqlite)))
                .map_err(|e| sql_err!(Metadata, e))?;
            trace!("query returned");
            while let Some(row) = rows.next().map_err(|e| sql_err!(Metadata, e))? {
                let mut map = Map::new();
                for (name, data_type) in &projected {
                    // Draft-Ableitungen und interne Spalten weiter unten separat behandeln.
                    let Some(dt) = data_type else { continue };
                    let value = row
                        .get_ref(name.as_str())
                        .map_err(|e| sql_err!(Metadata, e))?;
                    map.insert(name.clone(), param_to_json(&from_sqlite(value), dt));
                }
                debug!("Datenzeile als Json: {:#?}", map);
                let mut admin_id = None;
                let mut messages = None;
                if set.draft_enabled {
                    // Nicht alle Draft-Spalten sind zwingend in der Projektion:
                    // $select kann sie explizit weglassen. Deshalb nur setzen,
                    // wenn tatsaechlich im Result-Set vorhanden.
                    let has = |n: &str| projected.iter().any(|(name, _)| name == n);
                    if has("HasActiveEntity") {
                        map.insert(
                            "HasActiveEntity".into(),
                            json!(
                                row.get::<_, i64>("HasActiveEntity")
                                    .map_err(|e| sql_err!(Metadata, e))?
                                    != 0
                            ),
                        );
                    }
                    if has("HasDraftEntity") {
                        map.insert(
                            "HasDraftEntity".into(),
                            json!(
                                row.get::<_, i64>("HasDraftEntity")
                                    .map_err(|e| sql_err!(Metadata, e))?
                                    != 0
                            ),
                        );
                    }
                    if has("DraftAdministrativeData_ID") {
                        admin_id = row.get("DraftAdministrativeData_ID").ok();
                    }
                    if has("DraftMessages") {
                        messages = row.get("DraftMessages").ok();
                    }
                }
                raw.push((map, admin_id, messages));
            }
            debug!("raw hat {} Zeilen", raw.len());
        }

        raw.into_iter()
            .map(|(map, admin, messages)| self.finish_record(conn, map, admin, messages))
            .collect()
    }

    // -- Kompositionen --------------------------------------------------------

    /// Schluessel der Kompositionskinder **im selben Aktivierungszustand**
    /// wie der Parent.
    fn child_keys(
        &self,
        conn: &Connection,
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

            let mut params = vec![SqlParam::Text(key.storage_id())];
            if child.draft_enabled {
                sql.push_str(&format!(" AND {} = ?2", quote(IS_ACTIVE_COLUMN)));
                params.push(SqlParam::Bool(key.is_active_entity));
            }
            debug!("child keys: {}", sql);
            let mut stmt = conn.prepare(&sql).map_err(|e| sql_err!(Metadata, e))?;
            let ids = stmt
                .query_map(params_from_iter(params.iter().map(to_sqlite)), |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|e| sql_err!(Metadata, e))?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|e| sql_err!(Metadata, e))?;
            debug!("number of ids: {}", ids.len());
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

    fn delete_row(
        &self,
        conn: &Connection,
        set: &EntitySet,
        key: &EntityKey,
    ) -> Result<(), StoreError> {
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
        conn.execute(
            &sql,
            params_from_iter(Self::key_params(set, key).iter().map(to_sqlite)),
        )
        .map_err(|e| sql_err!(Metadata, e))?;
        Ok(())
    }

    fn draft_edit_tree(
        &self,
        conn: &Connection,
        set: &EntitySet,
        key: &EntityKey,
    ) -> Result<(), StoreError> {
        let mut record = self.load_one(conn, set, key)?;
        record.edit();
        self.write_record(conn, set, &record)?;
        let schema = self.schema();
        for (child_set, child_key) in self.child_keys(conn, set, key)? {
            let child = schema
                .set(&child_set)
                .map_err(|e| StoreError::Metadata(e.to_string()))?;
            self.draft_edit_tree(conn, child, &child_key)?;
        }
        Ok(())
    }

    /// Aktiviert einen Draft samt Kompositionskindern.
    ///
    /// Der Parent wird zuerst als aktiv geschrieben, sonst laufen die Kinder
    /// in ihren Fremdschluessel. Erst danach faellt die Draft-Zeile.
    fn draft_activate_tree(
        &self,
        conn: &Connection,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<Value, StoreError> {
        if key.is_active_entity {
            return Err(StoreError::InvalidState);
        }
        debug!("activate record");
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        let mut record = self.load_one(conn, set, key)?;
        record.activate()?;
        self.write_record(conn, set, &record)?;
        debug!("activate all children");
        for (child_set, child_key) in self.child_keys(conn, set, key)? {
            debug!("activate children");
            self.draft_activate_tree(conn, &child_set, &child_key)?;
        }

        self.delete_row(conn, set, key)?;
        Ok(json!(record))
    }

    // -- $expand --------------------------------------------------------------

    fn expand_rows(
        &self,
        conn: &Connection,
        set: &EntitySet,
        rows: &mut [DataRecord],
        query: &ODataQuery,
        strict: bool,
    ) -> Result<(), StoreError> {
        trace!("expanding rows {:?}", query.expand);
        if query.expand.is_empty() || rows.is_empty() {
            return Ok(());
        }
        for item in query.expand.iter() {
            let name = &item.navigation_property;
            if name == data_store::DRAFT_ADMIN_SET {
                debug!("special draft handling");
                for row in rows.iter_mut() {
                    if let Some(Value::String(draft_uuid)) = row.get("DraftAdministrativeData_ID") {
                        let admin = self.load_draft_admin(conn, &draft_uuid)?;
                        debug!("draft id {} -> {:?}", draft_uuid, admin);
                        row[name] = json!(admin);
                    }
                }
                continue;
            }
            match set.get_association(name) {
                Ok(assoc) => self.expand_association(conn, rows, assoc)?,
                Err(e) if strict => return Err(StoreError::Metadata(e.to_string())),
                Err(_) => {}
            }
        }
        Ok(())
    }

    fn expand_association(
        &self,
        conn: &Connection,
        rows: &mut [DataRecord],
        assoc: &Association,
    ) -> Result<(), StoreError> {
        info!("expand association");
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
                // Vollstaendige Abfrage ueber den Translator zusammenbauen,
                // damit die Draft-Wrapper (Sibling-Join, HasActiveEntity, ...)
                // konsistent zum Haupt-SELECT sind. Spalten werden ohne Alias
                // referenziert -- das aeussere SELECT hat keinen `AS t`.
                let raw = format!(
                    "{} IN ({})",
                    quote(ID_COLUMN),
                    placeholders(self.dialect(), ids.len(), 0)
                );
                let params: Vec<SqlParam> = ids.iter().cloned().map(SqlParam::Text).collect();
                let sql = Translator::new(target, self.dialect())
                    .filter_raw(raw, params)
                    .shadow_drafts()
                    .build()?;
                let found = self.query_records(conn, target, sql.text, sql.params, None, "")?;
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
                let raw = format!(
                    "{} IN ({})",
                    quote(fk),
                    placeholders(self.dialect(), ids.len(), 0)
                );
                let params: Vec<SqlParam> = ids.iter().cloned().map(SqlParam::Text).collect();
                let sql = Translator::new(target, self.dialect())
                    .filter_raw(raw, params)
                    .shadow_drafts()
                    .build()?;
                let found = self.query_records(conn, target, sql.text, sql.params, None, "")?;

                let mut grouped: HashMap<String, Vec<Value>> = HashMap::new();
                for record in found.iter() {
                    if let Some(parent) =
                        record.get(fk).and_then(|v| v.as_str().map(str::to_string))
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

    /// Schreibt den In-Memory-Stand auf die Platte (`<root>/data.sqlite`).
    ///
    /// Der Betrieb laeuft ausschliesslich im Speicher; diese Funktion ist der
    /// einzige Weg, Aenderungen dauerhaft zu machen. Aufrufer (Shutdown,
    /// Import) muessen sie selbst ausfuehren. Ohne Dateipfad ist sie ein No-op.
    ///
    /// Die Datei wird zuerst als `data.sqlite.tmp` geschrieben und dann atomar
    /// umbenannt, damit ein Abbruch keine halbe Datenbank hinterlaesst.
    pub fn persist(&self) -> anyhow::Result<()> {
        let Some(path) = &self.path else {
            warn!("no path specified for persisting the database");
            return Ok(());
        };
        let tmp = path.with_extension("sqlite.tmp");
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("store lock poisoned"))?;
        // Alte Temp-Datei aus einem abgebrochenen Lauf entfernen.
        let _ = fs::remove_file(&tmp);
        {
            let mut disk = Connection::open(&tmp)?;
            let backup = Backup::new(&*conn, &mut disk)?;
            backup.run_to_completion(256, Duration::ZERO, None)?;
            info!("memory database persisted to {}", tmp.display());
        }
        fs::rename(&tmp, path)?;
        info!("file {} renamed from ", path.display());
        Ok(())
    }
}

#[async_trait::async_trait]
impl DataStore for RelationalStore {
    fn component_info(&self) -> String {
        format!("RelationalStore {:?}", self.path)
    }
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

    async fn get_collection(
        &self,
        set_name: &str,
        query: &ODataQuery,
    ) -> Result<Vec<DataRecord>, StoreError> {
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");

        // Ein Draft verdeckt seinen aktiven Zwilling. Die Regel haengt am
        // Translator, nicht mehr an einer korrelierten Unterabfrage: der
        // Sibling-Join hat den Zustand des Zwillings bereits in der Zeile.
        // Sie ist wortgleich mit dem Draft-Filter, den Fiori Elements selbst
        // schickt, und damit idempotent.
        let mut sql = Translator::new(set, self.dialect())
            .with_schema(&schema)
            .shadow_drafts();
        for s in query.select.iter() {
            sql = sql.select(s);
        }
        // Fremdschluessel fuer $expand mitlesen, auch wenn $select sie nicht nennt.
        let helper_cols: Vec<String> = if query.select.is_empty() {
            vec![]
        } else {
            data_store::query::expand_key_columns(set, query)
                .into_iter()
                .filter(|c| !query.select.contains(c))
                .collect()
        };
        for c in helper_cols.iter() {
            sql = sql.select(c);
        }
        if !sql.select_is_empty() {
            sql = sql.select("DraftAdministrativeData_ID");
        }
        if let Some(filter) = &query.filter {
            sql = sql.filter(filter);
        }
        let sql = sql.build()?;
        debug!("build returns: {}", &sql.text);

        let conn = self.conn.lock().expect("connection lock");
        let mut records = self.query_records(
            &conn,
            set,
            sql.text,
            sql.params,
            order_by_clause(set, self.dialect(), query)?,
            &paging_clause(self.dialect(), query),
        )?;

        for record in records.iter_mut() {
            for s in query.select.iter() {
                record.touch(s);
            }
        }
        self.expand_rows(&conn, set, &mut records, query, false)?;
        for record in records.iter_mut() {
            for c in helper_cols.iter() {
                record.remove_field(c);
            }
        }
        Ok(records)
    }

    async fn count(&self, set_name: &str, query: &ODataQuery) -> usize {
        let mut q = data_store::query::count_query(query);
        q.orderby.clear();
        self.get_collection(set_name, &q)
            .await
            .map(|r| r.len())
            .unwrap_or(0)
    }

    async fn read_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        query: &ODataQuery,
    ) -> Result<DataRecord, StoreError> {
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        let conn = self.conn.lock().expect("connection lock");
        let mut rows = [self.load_one(&conn, set, key)?];
        self.expand_rows(&conn, set, &mut rows, query, true)?;
        let [row] = rows;
        Ok(row)
    }

    async fn create_entity(&self, set_name: &str, data: &Value) -> Result<DataRecord, StoreError> {
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
                        "ID" | "IsActiveEntity"
                            | "HasActiveEntity"
                            | "HasDraftEntity"
                            | "DraftAdministrativeData"
                            | "DraftMessages"
                    )
                    || set.get_association(field).is_ok();
                if !known {
                    return Err(StoreError::PropertyNotFound(format!("{set_name}.{field}")));
                }
            }
        }

        // Pruefung des Parents und das Schreiben gehoeren zusammen: sonst
        // koennte der Parent dazwischen aktiviert oder geloescht werden.
        in_transaction!(self, |conn| {
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
                let parent_record = self.load_one(&conn, parent_set, &parent_key)?;

                match (record.is_active_entity(), parent_record.draft_admin_data()) {
                    (true, None) => {}
                    (true, Some(_)) => return Err(StoreError::InvalidState),
                    (false, None) => return Err(StoreError::InvalidState),
                    (false, Some(admin)) => record.set_draft_uuid(admin.draft_uuid),
                }
            } else if !record.is_active_entity() {
                record.create_draft();
            }

            self.write_record(&conn, set, &record)?;
            Ok::<_, StoreError>(())
        });
        Ok(record)
    }

    async fn patch_entity(
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
                if column.is_immutable_on_change() {
                    return Err(StoreError::Conflict(format!(
                        "{set_name}.{field} ist unveraenderlich"
                    )));
                }
            }
        }
        // Lesen, Zusammenfuehren und Schreiben sind ein Vorgang - sonst
        // ueberschreibt der spaetere Patch die Aenderung des frueheren.
        in_transaction!(self, |conn| {
            let mut record = self.load_one(&conn, set, key)?;
            record.merge(patch);
            self.write_record(&conn, set, &record)?;
            Ok::<_, StoreError>(())
        });
        Ok(patch.clone())
    }

    async fn delete_entity(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        // Ein Draft verwirft immer auch seinen aktiven Zwilling; beide
        // Loeschungen gelten gemeinsam oder gar nicht.
        in_transaction!(self, |conn| {
            self.delete_row(&conn, set, key)?;
            if !key.is_active_entity {
                self.delete_row(&conn, set, &key.sibling())?;
            }
            Ok::<_, StoreError>(())
        });
        Ok(())
    }

    async fn delete_entity_tree_only(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<(), StoreError> {
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        in_transaction!(self, |conn| {
            self.delete_row(&conn, set, key)?;
            Ok::<_, StoreError>(())
        });
        Ok(())
    }

    async fn read_sibling_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<DataRecord, StoreError> {
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        let conn = self.conn.lock().expect("connection lock");
        self.load_one(&conn, set, &key.sibling())
    }

    async fn get_property(
        &self,
        set_name: &str,
        key: &EntityKey,
        property: &str,
    ) -> Result<Value, StoreError> {
        self.read_entity(set_name, key, &ODataQuery::new())
            .await?
            .get(property)
            .ok_or_else(|| StoreError::PropertyNotFound(property.into()))
    }

    async fn commit(&self) {}
}

#[async_trait::async_trait]
impl DraftDataStore for RelationalStore {
    async fn draft_edit(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        if !key.is_active_entity {
            return Err(StoreError::InvalidState);
        }
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        // Der ganze Teilbaum geht gemeinsam in den Draft. Ein Abbruch in der
        // Mitte hinterliesse einen Draft-Parent mit aktiven Kindern.
        let record = in_transaction!(self, |conn| {
            let mut record = self.load_one(&conn, set, key)?;

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
                    .load_one(&conn, parent_set, &EntityKey::new(parent_uuid, false))
                    .is_err()
                {
                    return Err(StoreError::InvalidState);
                }
            }

            record.edit();
            self.write_record(&conn, set, &record)?;
            for (child_set, child_key) in self.child_keys(&conn, set, key)? {
                let child = schema
                    .set(&child_set)
                    .map_err(|e| StoreError::Metadata(e.to_string()))?;
                self.draft_edit_tree(&conn, child, &child_key)?;
            }
            Ok::<_, StoreError>(record)
        });
        Ok(json!(record))
    }

    /// Draft Datenatz aktivieren und die potenziellen Kinder auch aktivieren.
    /// Der Elternsatz muss als erstes als aktiv eingefügt werden, weil die
    /// Kinder sonst einen Foreign-Key Fehler bekommen, da sie ja auf die
    /// Eltern zeigen.
    async fn draft_activate(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        if key.is_active_entity {
            return Err(StoreError::InvalidState);
        }
        debug!("draft activate");
        // Eine Aktivierung ist entweder vollstaendig oder gar nicht
        // geschehen. Scheitert ein Kind, darf der Parent nicht aktiv
        // zurueckbleiben.
        Ok(in_transaction!(self, |conn| {
            self.draft_activate_tree(&conn, set_name, key)
        }))
    }

    async fn draft_prepare(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        let conn = self.conn.lock().expect("connection lock");
        Ok(json!(self.load_one(&conn, set, key)?))
    }
}
