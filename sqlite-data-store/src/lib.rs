//! SQLite-Implementierung des in `data-store` definierten Interfaces.
//!
//! Die Abstraktion (Traits, `DataRecord`, `EntityKey`, ...) lebt in `data-store`;
//! dieses Crate liefert die SQLite-spezifische Ablage.

// Re-export der abstrakten Schnittstelle, damit Nutzer nur ein Crate brauchen.
pub use data_store::{
    DataRecord, DataStore, DraftAction, DraftAdministrativeData, DraftDataStore, DraftMessage,
    EntityKey, Navigation, StoreError,
};

pub mod sqlite;
pub use sqlite::SqliteStore;

#[cfg(test)]
mod tests;
