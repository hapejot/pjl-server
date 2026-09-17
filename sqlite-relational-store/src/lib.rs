//! Relationale SQLite-Implementierung des `data-store`-Interfaces.
//!
//! Im Gegensatz zu den Dokumenten-Backends (`odata-yaml-store`,
//! `sqlite-data-store`) liegt hier **eine Tabelle je Entity-Set** mit echten,
//! typisierten Spalten. Das Schema kommt ueber [`data_store::schema::SchemaSource`]
//! als Konstruktor-Abhaengigkeit - nicht durch den `DataStore`-Trait.
//!
//! Kernpunkte:
//!
//! * Drafts und aktive Saetze teilen sich eine Tabelle,
//!   Primaerschluessel `(ID, IsActiveEntity)`.
//! * Kompositionen sind echte Fremdschluessel mit `ON DELETE CASCADE`,
//!   inklusive `IsActiveEntity` - ein aktiver Parent kann strukturell kein
//!   Draft-Kind haben.
//! * `HasActiveEntity`/`HasDraftEntity` werden berechnet statt gespeichert.
//! * `$filter`/`$orderby` werden nach SQL uebersetzt; nicht Uebersetzbares
//!   liefert einen Fehler statt stiller Abweichung.

pub mod catalog;
pub mod ddl;
pub mod dialect;
pub mod sql;
pub mod store;
pub mod value;

pub use data_store::schema::{Schema, SchemaError, SchemaSource, StaticSchema};
pub use data_store::{
    DataRecord, DataStore, DraftAdministrativeData, DraftDataStore, DraftMessage, EntityKey,
    StoreError,
};
pub use store::RelationalStore;
