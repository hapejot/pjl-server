//! Relationaler Postgres-Store: eine Tabelle je Entity-Set.
//!
//! Der Zusammenbau der Abfragen kommt vollstaendig aus `relational-core`;
//! hier steht nur, was der Treiber selbst betrifft - Verbindung, Binden der
//! Parameter, Lesen der Zeilen, Transaktionen.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use data_store::schema::{
    Association, Cardinality, EntitySet, Schema, SchemaSource, ID_COLUMN, IS_ACTIVE_COLUMN,
};
use data_store::{
    json_to_param, param_to_json, DataRecord, DataStore, DraftDataStore, EntityKey, Navigation,
    SqlParam, StoreError,
};
use odata::query::{Expr, ODataQuery};
use deadpool_postgres::{Object, Pool};
use tokio_postgres::types::ToSql;
use deadpool_postgres::GenericClient;
use tokio_postgres::NoTls;
use relational_core::ddl::{
    self, quote, DRAFT_ADMIN_COLUMN, DRAFT_ADMIN_TABLE, DRAFT_MESSAGES_COLUMN,
};
use relational_core::dialect::{Dialect, POSTGRES};
use relational_core::sql::query::{order_by_clause, Translator};
use relational_core::sql::{paging_clause, placeholders};
use serde_json::{json, Map, Value};
use tracing::{debug, trace};
use uuid::Uuid;

use crate::catalog::PostgresCatalog;
use crate::value::{from_postgres, PgParam};

macro_rules! sql_err {
    ($variant:ident, $e:expr) => {
        StoreError::$variant(format!("SQL ({}:{}): {}", module_path!(), line!(), $e))
    };
}

/// Eine Verbindung aus dem Pool.
///
/// Anders als bei SQLite gibt es keinen Mutex auf einer einzelnen Verbindung:
/// nebenlaeufige Anfragen bekommen jeweils eine eigene und laufen wirklich
/// parallel.
macro_rules! conn {
    ($self:expr) => {
        $self
            .pool
            .get()
            .await
            .map_err(|e| sql_err!(Conversion, e))?
    };
}

/// Fuehrt `body` in einer Transaktion aus.
///
/// Anders als bei SQLite, wo ein Mutex die Zugriffe ohnehin serialisiert,
/// laufen hier echte Nebenlaeufer gegen dieselbe Datenbank. Ein abgebrochener
/// Schreibvorgang darf keinen halben Zustand hinterlassen.
macro_rules! in_transaction {
    ($self:expr, |$tx:ident| $body:block) => {{
        let mut connection = conn!($self);
        let $tx = connection
            .transaction()
            .await
            .map_err(|e| sql_err!(Conversion, e))?;
        let result = async { $body }.await?;
        $tx.commit().await.map_err(|e| sql_err!(Conversion, e))?;
        result
    }};
}

/// Parameterliste in die Form bringen, die der Treiber erwartet.
fn bind(params: &[SqlParam]) -> Vec<PgParam<'_>> {
    params.iter().map(PgParam).collect()
}

fn as_refs<'a>(params: &'a [PgParam<'a>]) -> Vec<&'a (dyn ToSql + Sync)> {
    params.iter().map(|p| p as &(dyn ToSql + Sync)).collect()
}

pub struct PostgresStore {
    pool: Pool,
    schema_source: Arc<dyn SchemaSource>,
    schema: Mutex<Arc<Schema>>,
}

impl PostgresStore {
    /// Verbindet sich und bringt die Datenbank auf den Stand des Schemas.
    pub async fn connect(
        url: &str,
        schema_source: Arc<dyn SchemaSource>,
    ) -> Result<Self, StoreError> {
        let config: tokio_postgres::Config = url.parse().map_err(|e| sql_err!(Metadata, e))?;
        let manager = deadpool_postgres::Manager::from_config(
            config,
            NoTls,
            deadpool_postgres::ManagerConfig {
                recycling_method: deadpool_postgres::RecyclingMethod::Fast,
            },
        );
        let pool = Pool::builder(manager)
            .max_size(16)
            .build()
            .map_err(|e| sql_err!(Metadata, e))?;
        Self::from_pool(pool, schema_source).await
    }

    /// Uebernimmt einen bereits aufgebauten Pool.
    pub async fn from_pool(
        pool: Pool,
        schema_source: Arc<dyn SchemaSource>,
    ) -> Result<Self, StoreError> {
        let schema = schema_source
            .schema()
            .map_err(|e| StoreError::Metadata(e.to_string()))?;
        {
            let mut client = pool.get().await.map_err(|e| sql_err!(Metadata, e))?;
            apply_schema(&mut client, &POSTGRES, &schema).await?;
            set_applied_revision(&client, &POSTGRES, schema_source.revision().0).await?;
        }
        Ok(Self {
            pool,
            schema_source,
            schema: Mutex::new(schema),
        })
    }

    /// Uebernimmt einen neuen Schemastand und migriert die Datenbank.
    pub async fn refresh_schema(&self) -> Result<bool, StoreError> {
        let revision = self.schema_source.revision();
        let mut client = conn!(self);
        if applied_revision(&client).await == Some(revision.0) {
            return Ok(false);
        }
        let schema = self
            .schema_source
            .schema()
            .map_err(|e| StoreError::Metadata(e.to_string()))?;
        apply_schema(&mut client, &POSTGRES, &schema).await?;
        set_applied_revision(&client, &POSTGRES, revision.0).await?;
        *self.schema.lock().expect("schema lock") = schema;
        Ok(true)
    }

    /// Spalten, die in der Datenbank stehen, aber nicht mehr im Schema.
    pub async fn pending_destructive_changes(&self) -> Result<Vec<String>, StoreError> {
        let client = conn!(self);
        let schema = self.schema();
        ddl::pending_destructive_changes(&PostgresCatalog::new(&client), &POSTGRES, &schema).await
    }

    fn dialect(&self) -> &'static dyn Dialect {
        &POSTGRES
    }

    pub fn schema(&self) -> Arc<Schema> {
        self.schema.lock().expect("schema lock").clone()
    }

    fn entity_set(&self, set_name: &str) -> Result<Arc<Schema>, StoreError> {
        let schema = self.schema();
        schema
            .set(set_name)
            .map_err(|e| StoreError::Metadata(e.to_string()))?;
        Ok(schema)
    }

    /// Anzahl gespeicherter Saetze je Aktivierungszustand.
    pub async fn count_state(&self, set_name: &str, is_active: bool) -> usize {
        let Ok(schema) = self.entity_set(set_name) else {
            return 0;
        };
        let set = schema.set(set_name).expect("geprueft");
        let Ok(client) = self.pool.get().await else {
            return 0;
        };
        let result = if set.draft_enabled {
            client
                .query_one(
                    &format!(
                        "SELECT COUNT(*) FROM {} WHERE {} = $1",
                        quote(set_name),
                        quote(IS_ACTIVE_COLUMN)
                    ),
                    &[&is_active],
                )
                .await
        } else if is_active {
            client
                .query_one(&format!("SELECT COUNT(*) FROM {}", quote(set_name)), &[])
                .await
        } else {
            return 0;
        };
        result.map(|r| r.get::<_, i64>(0)).unwrap_or(0) as usize
    }

    /// Anzahl der Draft-Verwaltungssaetze. Diagnose- und Testhilfe.
    pub async fn count_draft_admin(&self) -> usize {
        let Ok(client) = self.pool.get().await else {
            return 0;
        };
        client
            .query_one(
                &format!("SELECT COUNT(*) FROM {}", quote(DRAFT_ADMIN_TABLE)),
                &[],
            )
            .await
            .map(|r| r.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }

    // -- SELECT-Bausteine ----------------------------------------------------

    async fn load_draft_admin(
        &self,
        conn: &impl GenericClient,
        draft_uuid: &str,
    ) -> Result<Option<Value>, StoreError> {
        let rows = conn
            .query(
                &format!(
                    "SELECT {uuid}, {created}, {in_process}, {changed} FROM {table} \
                     WHERE {uuid} = $1",
                    uuid = quote("DraftUUID"),
                    created = quote("DraftIsCreatedByMe"),
                    in_process = quote("InProcessByUser"),
                    changed = quote("LastChangedByUser"),
                    table = quote(DRAFT_ADMIN_TABLE),
                ),
                &[&draft_uuid],
            ).await
            .map_err(|e| sql_err!(Metadata, e))?;
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        Ok(Some(json!({
            "DraftUUID": row.get::<_, String>(0),
            "DraftIsCreatedByMe": row.get::<_, bool>(1),
            "InProcessByUser": row.get::<_, Option<String>>(2).unwrap_or_default(),
            "LastChangedByUser": row.get::<_, Option<String>>(3).unwrap_or_default(),
        })))
    }

    async fn store_draft_admin(
        &self,
        conn: &impl GenericClient,
        record: &DataRecord,
    ) -> Result<Option<String>, StoreError> {
        let Some(admin) = record.draft_admin_data() else {
            return Ok(None);
        };
        let uuid = admin.draft_uuid.to_string();
        conn.execute(
            &format!(
                "INSERT INTO {table} ({uuid}, {created}, {in_process}, {changed}) \
                 VALUES ($1, $2, $3, $4) \
                 ON CONFLICT ({uuid}) DO UPDATE SET \
                 {created} = excluded.{created}, \
                 {in_process} = excluded.{in_process}, \
                 {changed} = excluded.{changed}",
                table = quote(DRAFT_ADMIN_TABLE),
                uuid = quote("DraftUUID"),
                created = quote("DraftIsCreatedByMe"),
                in_process = quote("InProcessByUser"),
                changed = quote("LastChangedByUser"),
            ),
            &[
                &uuid,
                &admin.draft_is_created_by_me,
                &admin.in_process_by_user,
                &admin.last_changed_by_user,
            ],
        ).await
        .map_err(|e| sql_err!(Metadata, e))?;
        Ok(Some(uuid))
    }

    /// Schreibt einen Datensatz (INSERT ... ON CONFLICT DO UPDATE).
    async fn write_record(
        &self,
        conn: &impl GenericClient,
        set: &EntitySet,
        record: &DataRecord,
    ) -> Result<(), StoreError> {
        // Laeuft in derselben Transaktion wie der Satz selbst.
        let admin_id = self.store_draft_admin(conn, record).await?;

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
            vals = placeholders(self.dialect(), columns.len(), 0),
            keys = key_columns.join(", "),
            sets = if assignments.is_empty() {
                format!("{0} = excluded.{0}", quote(ID_COLUMN))
            } else {
                assignments.join(", ")
            },
        );
        trace!("write: {sql}");
        let bound = bind(&values);
        conn.execute(&sql, &as_refs(&bound)).await
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
    async fn load_one(
        &self,
        conn: &impl GenericClient,
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
        let records = self.query_records(conn, set, q.text, q.params, None, "").await?;
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

    async fn finish_record(
        &self,
        conn: &impl GenericClient,
        mut map: Map<String, Value>,
        admin_id: Option<String>,
        messages: Option<String>,
    ) -> Result<DataRecord, StoreError> {
        if let Some(admin_id) = admin_id {
            if let Some(admin) = self.load_draft_admin(conn, &admin_id).await? {
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
    async fn query_records(
        &self,
        conn: &impl GenericClient,
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

        let bound = bind(&params);
        let rows = conn
            .query(&sql, &as_refs(&bound)).await
            .map_err(|e| sql_err!(Metadata, format!("{e} in {sql}")))?;

        // Nur die tatsaechlich projizierten Spalten materialisieren: $select
        // verkleinert die SELECT-Liste, also ist der Result-Set die Wahrheit.
        // Die Zuordnung erfolgt namensbasiert.
        let mut raw: Vec<(Map<String, Value>, Option<String>, Option<String>)> = vec![];
        for row in &rows {
            let mut map = Map::new();
            let mut admin_id = None;
            let mut messages = None;
            for (index, column) in row.columns().iter().enumerate() {
                let name = column.name().to_string();
                let value = from_postgres(row, index)?;
                match name.as_str() {
                    "HasActiveEntity" | "HasDraftEntity" => {
                        map.insert(name, param_to_json(&value, &data_store::DataType::Bool));
                    }
                    DRAFT_ADMIN_COLUMN => {
                        if let SqlParam::Text(id) = value {
                            admin_id = Some(id);
                        }
                    }
                    DRAFT_MESSAGES_COLUMN => {
                        if let SqlParam::Text(m) = value {
                            messages = Some(m);
                        }
                    }
                    _ => {
                        let Some(column) = set.columns().find(|c| c.name == name) else {
                            continue;
                        };
                        map.insert(name, param_to_json(&value, &column.data_type));
                    }
                }
            }
            raw.push((map, admin_id, messages));
        }

        let mut records = Vec::with_capacity(raw.len());
        for (map, admin, messages) in raw {
            records.push(self.finish_record(conn, map, admin, messages).await?);
        }
        Ok(records)
    }

    // -- Kompositionen --------------------------------------------------------

    /// Schluessel der Kompositionskinder **im selben Aktivierungszustand**
    /// wie der Parent.
    async fn child_keys(
        &self,
        conn: &impl GenericClient,
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
                "SELECT {} FROM {} WHERE {} = $1",
                quote(ID_COLUMN),
                quote(&child.name),
                quote(fk)
            );
            let mut params = vec![SqlParam::Text(key.storage_id())];
            if child.draft_enabled {
                sql.push_str(&format!(" AND {} = $2", quote(IS_ACTIVE_COLUMN)));
                params.push(SqlParam::Bool(key.is_active_entity));
            }
            let bound = bind(&params);
            let rows = conn
                .query(&sql, &as_refs(&bound)).await
                .map_err(|e| sql_err!(Metadata, e))?;
            for row in rows {
                let id: String = row.get(0);
                let uuid = Uuid::parse_str(&id).map_err(|e| StoreError::Conflict(e.to_string()))?;
                result.push((
                    child.name.clone(),
                    EntityKey::new(uuid, key.is_active_entity),
                ));
            }
        }
        Ok(result)
    }

    async fn delete_row(
        &self,
        conn: &impl GenericClient,
        set: &EntitySet,
        key: &EntityKey,
    ) -> Result<(), StoreError> {
        // ON DELETE CASCADE raeumt die Kompositionskinder ab.
        let sql = format!(
            "DELETE FROM {} WHERE {} = $1{}",
            quote(&set.name),
            quote(ID_COLUMN),
            if set.draft_enabled {
                format!(" AND {} = $2", quote(IS_ACTIVE_COLUMN))
            } else {
                String::new()
            }
        );
        let params = Self::key_params(set, key);
        let bound = bind(&params);
        conn.execute(&sql, &as_refs(&bound)).await
            .map_err(|e| sql_err!(Metadata, e))?;
        Ok(())
    }

    /// Rekursiv, deshalb geboxt: eine `async fn`, die sich selbst aufruft,
    /// haette eine Zukunft von unbestimmter Groesse.
    fn draft_edit_tree<'a>(
        &'a self,
        conn: &'a impl GenericClient,
        set: &'a EntitySet,
        key: &'a EntityKey,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), StoreError>> + Send + 'a>>
    {
        Box::pin(async move {
        let mut record = self.load_one(conn, set, key).await?;
        record.edit();
        self.write_record(conn, set, &record).await?;
        let schema = self.schema();
        for (child_set, child_key) in self.child_keys(conn, set, key).await? {
            let child = schema
                .set(&child_set)
                .map_err(|e| StoreError::Metadata(e.to_string()))?;
            self.draft_edit_tree(conn, child, &child_key).await?;
        }
        Ok(())
        })
    }

    /// Aktiviert einen Draft samt Kompositionskindern.
    ///
    /// Der Parent wird zuerst als aktiv geschrieben, sonst laufen die Kinder
    /// in ihren Fremdschluessel. Erst danach faellt die Draft-Zeile.
    fn draft_activate_tree<'a>(
        &'a self,
        conn: &'a impl GenericClient,
        set_name: &'a str,
        key: &'a EntityKey,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, StoreError>> + Send + 'a>>
    {
        Box::pin(async move {
        if key.is_active_entity {
            return Err(StoreError::InvalidState);
        }
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        let mut record = self.load_one(conn, set, key).await?;
        record.activate()?;
        self.write_record(conn, set, &record).await?;

        for (child_set, child_key) in self.child_keys(conn, set, key).await? {
            self.draft_activate_tree(conn, &child_set, &child_key).await?;
        }

        self.delete_row(conn, set, key).await?;
        Ok(json!(record))
        })
    }

    // -- $expand --------------------------------------------------------------

    async fn expand_rows(
        &self,
        conn: &impl GenericClient,
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
                Ok(assoc) => self.expand_association(conn, rows, assoc).await?,
                Err(e) if strict => return Err(StoreError::Metadata(e.to_string())),
                Err(_) => {}
            }
        }
        Ok(())
    }

    async fn expand_association(
        &self,
        conn: &impl GenericClient,
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
                let found = self.query_records(conn, target, sql.text, sql.params, None, "").await?;
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
                let found = self.query_records(conn, target, sql.text, sql.params, None, "").await?;

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

/// Setzt die von [`ddl::migration_plan`] geplanten Anweisungen ab.
async fn apply_schema(
    client: &mut Object,
    dialect: &dyn Dialect,
    schema: &Schema,
) -> Result<(), StoreError> {
    let plan = ddl::migration_plan(&PostgresCatalog::new(&*client), dialect, schema).await?;
    for statement in plan {
        debug!("DDL: {statement}");
        client
            .batch_execute(&statement)
            .await
            .map_err(|e| sql_err!(Conversion, format!("{e} in {statement}")))?;
    }
    Ok(())
}

async fn applied_revision(client: &impl GenericClient) -> Option<u64> {
    client
        .query_one(&ddl::revision_select_sql(), &[])
        .await
        .ok()
        .map(|row| row.get::<_, i64>(0) as u64)
}

async fn set_applied_revision(
    client: &impl GenericClient,
    dialect: &dyn Dialect,
    revision: u64,
) -> Result<(), StoreError> {
    client
        .execute(&ddl::revision_delete_sql(), &[])
        .await
        .map_err(|e| sql_err!(Conversion, e))?;
    client
        .execute(&ddl::revision_insert_sql(dialect), &[&(revision as i64)])
        .await
        .map_err(|e| sql_err!(Conversion, e))?;
    Ok(())
}

#[async_trait::async_trait]
impl DataStore for PostgresStore {
    /// Metadaten kommen ueber [`SchemaSource`], nicht ueber den Store-Trait.
    fn navigation(&mut self, _single: bool) -> &mut Navigation {
        panic!(
            "PostgresStore bezieht seine Metadaten aus einer SchemaSource - \
             DataStore::navigation() wird nicht unterstuetzt"
        )
    }

    fn set_navigations(&self, _navigations: Vec<Navigation>) {
        debug!("set_navigations() ignoriert - PostgresStore nutzt die SchemaSource");
    }

    async fn get_collection(
        &self,
        set_name: &str,
        query: &ODataQuery,
    ) -> Result<Vec<DataRecord>, StoreError> {
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");

        let mut sql = Translator::new(set, self.dialect()).shadow_drafts();
        for s in query.select.iter() {
            sql = sql.select(s);
        }
        if let Some(filter) = &query.filter {
            sql = sql.filter(filter);
        }
        let sql = sql.build()?;

        let client = conn!(self);
        let mut records = self.query_records(
            &client,
            set,
            sql.text,
            sql.params,
            order_by_clause(set, self.dialect(), query)?,
            &paging_clause(self.dialect(), query),
        ).await?;

        for record in records.iter_mut() {
            for s in query.select.iter() {
                record.touch(s);
            }
        }
        self.expand_rows(&client, set, &mut records, query, false).await?;
        Ok(records)
    }

    async fn count(&self, set_name: &str, query: &ODataQuery) -> usize {
        let mut q = data_store::query::count_query(query);
        q.orderby.clear();
        self.get_collection(set_name, &q).await
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
        let client = conn!(self);
        let mut rows = [self.load_one(&client, set, key).await?];
        self.expand_rows(&client, set, &mut rows, query, true).await?;
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

        in_transaction!(self, |tx| {
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
                let parent_record = self.load_one(&tx, parent_set, &parent_key).await?;

                match (record.is_active_entity(), parent_record.draft_admin_data()) {
                    (true, None) => {}
                    (true, Some(_)) => return Err(StoreError::InvalidState),
                    (false, None) => return Err(StoreError::InvalidState),
                    (false, Some(admin)) => record.set_draft_uuid(admin.draft_uuid),
                }
            } else if !record.is_active_entity() {
                record.create_draft();
            }

            self.write_record(&tx, set, &record).await?;
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
        in_transaction!(self, |tx| {
            let mut record = self.load_one(&tx, set, key).await?;
            record.merge(patch);
            self.write_record(&tx, set, &record).await?;
            Ok::<_, StoreError>(())
        });
        Ok(patch.clone())
    }

    async fn delete_entity(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        in_transaction!(self, |tx| {
            self.delete_row(&tx, set, key).await?;
            if !key.is_active_entity {
                self.delete_row(&tx, set, &key.sibling()).await?;
            }
            Ok::<_, StoreError>(())
        });
        Ok(())
    }

    async fn delete_entity_tree_only(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        in_transaction!(self, |tx| {
            self.delete_row(&tx, set, key).await?;
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
        let client = conn!(self);
        self.load_one(&client, set, &key.sibling()).await
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
impl DraftDataStore for PostgresStore {
    async fn draft_edit(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        if !key.is_active_entity {
            return Err(StoreError::InvalidState);
        }
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");

        let record = in_transaction!(self, |tx| {
            let mut record = self.load_one(&tx, set, key).await?;

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
                    .load_one(&tx, parent_set, &EntityKey::new(parent_uuid, false)).await
                    .is_err()
                {
                    return Err(StoreError::InvalidState);
                }
            }

            record.edit();
            self.write_record(&tx, set, &record).await?;
            for (child_set, child_key) in self.child_keys(&tx, set, key).await? {
                let child = schema
                    .set(&child_set)
                    .map_err(|e| StoreError::Metadata(e.to_string()))?;
                self.draft_edit_tree(&tx, child, &child_key).await?;
            }
            Ok::<_, StoreError>(record)
        });
        Ok(json!(record))
    }

    async fn draft_activate(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        if key.is_active_entity {
            return Err(StoreError::InvalidState);
        }
        Ok(in_transaction!(self, |tx| {
            self.draft_activate_tree(&tx, set_name, key).await
        }))
    }

    async fn draft_prepare(&self, set_name: &str, key: &EntityKey) -> Result<Value, StoreError> {
        let schema = self.entity_set(set_name)?;
        let set = schema.set(set_name).expect("geprueft");
        let client = conn!(self);
        Ok(json!(self.load_one(&client, set, key).await?))
    }
}
