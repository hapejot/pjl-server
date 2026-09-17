//! Relationale PostgreSQL-Implementierung des `data-store`-Interfaces.
//!
//! Aufbau und Semantik entsprechen `sqlite-relational-store`: eine Tabelle je
//! Entity-Set, Drafts und aktive Saetze mit Primaerschluessel
//! `(ID, IsActiveEntity)`, Kompositionen als Fremdschluessel mit
//! `ON DELETE CASCADE`. Die Uebereinstimmung ist kein Zufall - beide Backends
//! erzeugen ihr SQL mit `relational-core` und laufen gegen dieselbe
//! Verhaltenssuite.
//!
//! Unterschiede zu SQLite liegen nicht im Ablauf, sondern in den Typen:
//! `IsActiveEntity` ist hier ein echter `BOOLEAN`, und der Treiber prueft
//! streng. Beides faengt `relational_core::dialect::Postgres` ab.

pub mod catalog;
pub mod store;
pub mod value;

pub use relational_core::{ddl, dialect, sql};
pub use store::PostgresStore;

pub use data_store::schema::{Schema, SchemaError, SchemaSource, StaticSchema};
pub use data_store::{
    DataRecord, DataStore, DraftAdministrativeData, DraftDataStore, DraftMessage, EntityKey,
    StoreError,
};
