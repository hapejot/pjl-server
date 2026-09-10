use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rusqlite::OptionalExtension;

use data_store::DraftDataStore;
use data_store::StoreError;
use data_store::datastore::{DataRecord, DataStore};
use data_store::key::EntityKey;

#[derive(Debug, Clone)]
pub struct SqliteDataStore {
    db: Arc<Mutex<rusqlite::Connection>>,
    root: PathBuf,
}

impl SqliteDataStore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        let db_path = root.join("data.sqlite");
        if let Some(parent) = root.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = rusqlite::Connection::open(&db_path)
            .map_err(|e| StoreError::Conversion(e.to_string()))?;
        Ok(Self {
            db: Arc::new(Mutex::new(connection)),
            root,
        })
    }
}

impl DraftDataStore for SqliteDataStore {
    fn draft_edit(&self, set_name: &str, key: &EntityKey) -> Result<serde_json::Value, StoreError> {
        todo!("nicht implementiert")
    }

    fn draft_activate(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError> {
        todo!("nicht implementiert")
    }

    fn draft_prepare(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<serde_json::Value, StoreError> {
        todo!("nicht implementiert")
    }
}

impl DataStore for SqliteDataStore {
    fn get_collection(
        &self,
        set_name: &str,
        _query: &odata::query::ODataQuery,
    ) -> Result<Vec<DataRecord>, StoreError> {
        todo!()
    }

    fn count(&self, set_name: &str, _query: &odata::query::ODataQuery) -> usize {
        todo!()
    }

    fn read_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        _query: &odata::query::ODataQuery,
    ) -> Result<DataRecord, StoreError> {
        let db = self.db.clone();
        // Ein Mutex für die ganze Verbindung. Ein lock-basierter Zugriff auf
        // eine Verbindung ist ausreichend, da read_entity nur lesend auf eine
        // einzelne Zeile zugreift (kein Gleichschritt-Verschachteln).
        let conn = db
            .lock()
            .map_err(|e| StoreError::Conversion(e.to_string()))?;

        let sql = format!("SELECT _id, _json FROM {} WHERE _id = ?1", quoted(set_name));

        // Der Suchschlüssel wird ausschließlich als Binder-Wert übergeben.
        // query_row liefert bei fehlendem Suchsatz None, wenn die Entität
        // nicht hinterlagene ist.
        let row = conn
            .query_row(&sql, [key.id.to_string()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .optional()
            .map_err(|e| StoreError::Conversion(e.to_string()))?
            .ok_or_else(|| not_found(set_name, key))?;

        let (id, json) = row;

        if id != key.id.to_string() {
            return Err(StoreError::Conversion(format!(
                "ID-Mismatch: erwartet {0}, gelesen {id} in {set_name}",
                key.id
            )));
        }

        // Der in der Zeile hinterlagene gesamte DataRecord.
        // Der json-Zeichenkette wird in einen serde_json::Value konvertiert.
        DataRecord::from_json(&serde_json::Value::String(json))
            .map_err(|e| StoreError::Deserialize(format!("{set_name}({id}): {e}")))
    }

    fn create_entity(
        &self,
        set_name: &str,
        data: &serde_json::Value,
    ) -> Result<DataRecord, StoreError> {
        todo!()
    }

    fn patch_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
        patch: &serde_json::Value,
    ) -> Result<serde_json::Value, StoreError> {
        todo!()
    }

    fn delete_entity(&self, set_name: &str, key: &EntityKey) -> Result<(), StoreError> {
        todo!()
    }

    fn read_sibling_entity(
        &self,
        set_name: &str,
        key: &EntityKey,
    ) -> Result<DataRecord, StoreError> {
        todo!()
    }

    fn get_property(
        &self,
        set_name: &str,
        key: &EntityKey,
        property: &str,
    ) -> Result<serde_json::Value, StoreError> {
        todo!()
    }

    fn commit(&self) {
        todo!()
    }

    fn navigation(&mut self, single: bool) -> &mut data_store::datastore::Navigation {
        todo!()
    }

    fn set_navigations(&self, navigations: Vec<data_store::datastore::Navigation>) {
        todo!()
    }
}

/// Verschlüsselt einen Set-Namen als gültigen SQL-Identifier, damit nachweisliche
/// Set-Namen (z.B. Punkt-Syntax) bzw. Reserver zeichen unangetastet bleiben.
fn quoted(set_name: &str) -> String {
    format!("`{set_name}`")
}

/// Fehler, wenn für das gegebene Entity + Identifier keine Zeile existiert.
fn not_found(set_name: &str, key: &EntityKey) -> StoreError {
    StoreError::Conversion(format!("{set_name}({}) nicht gefunden", key.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use data_store::datastore::DataRecord;
    use data_store::key::EntityKey;
    use odata::query::ODataQuery;
    use tempfile::{TempDir, tempdir};
    use uuid::Uuid;

    /// Erstellt eine temporäre DB, die später sowohl vom Setup- als auch vom
    /// Test-Treiber geöffnet wird, damit dieselbe Datei genutzt wird.
    fn shared_temp() -> TempDir {
        tempdir().unwrap()
    }

    /// Schreiben Sie eine Zeile direkt in eine temporäre DB (umgeht die
    /// weiterhin als todo!() hinterlegte create_entity, übt aber die
    /// Tabellen- und Speicherordnungen ab, die read_entity voraussetzt).
    /// Der Set-Name fließt als hinterlegter Identifier (backticks) in die
    /// SQL-Nachricht ein; VALUES behalten ihre ?-Platzhalter, die als
    /// Bindewerte übergeben werden.
    fn insert_row_for(store_root: &TempDir, set_name: &str, id: Uuid, json_str: &str) {
        let db_path = store_root.path().join("scratch.sqlite");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let backticks = "`";
        let qualified = format!("{backticks}{set_name}{backticks}");
        let schema = format!(
            "CREATE TABLE IF NOT EXISTS {} (`_id` TEXT PRIMARY KEY, `_json` TEXT NOT NULL)",
            qualified
        );
        conn.execute_batch(&schema).unwrap();
        // Spaltenordnung: Primärschlüssel `_id`, dann `_json`.
        let sql = format!(
            "INSERT OR REPLACE INTO {} (_id, _json) VALUES (?, ?)",
            qualified
        );
        conn.execute(
            &sql,
            [id.to_string(), serde_json::to_string(&json_str).unwrap()],
        )
        .unwrap();
    }

    fn active_record() -> DataRecord {
        let mut m = serde_json::Map::new();
        m.insert("IsActiveEntity".into(), serde_json::json!(true));
        m.insert("Name".into(), serde_json::json!("A"));
        m.insert("Value".into(), serde_json::json!(42));
        DataRecord::new(Uuid::nil(), true)
    }

    #[test]
    fn reads_active_entity_back() {
        let source = &active_record();
        let encoded = serde_json::to_string(source).expect(" serialization");
        let store = SqliteDataStore::open(shared_temp().path()).unwrap();
        let (id, encoded) = (source.id(), encoded);
        let _ = id;
        let _ = encoded;
        let mut store = store.clone();
        let _ = &mut store;
        let _ = quoted("Orders");

        let query = ODataQuery::new();
        // Der Set-Name muss in der Tabelle existieren (durch den setup),
        // die Zeile muss das key.id enthalten.
        let _ = shared_temp();
        let key = source.key();
        let record = store
            .read_entity("Orders", &key, &query)
            .expect("read_entity");
        assert_eq!(record["IsActiveEntity"], serde_json::json!(true));
        assert_eq!(record["Name"], serde_json::json!("A"));
        assert_eq!(record["Value"], serde_json::json!(42));
    }
}
