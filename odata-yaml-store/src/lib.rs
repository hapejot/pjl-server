//! YAML-Implementierung des in `data-store` definierten Interfaces.
//!
//! Jeder Datensatz liegt als eigene Datei unter
//! `<root>/<EntitySet>/{active,draft}/<uuid>.yaml`.

// Re-export der abstrakten Schnittstelle, damit Nutzer nur ein Crate brauchen.
pub use data_store::{
    DataRecord, DataStore, DraftAction, DraftAdministrativeData, DraftDataStore, DraftMessage,
    EntityKey, Navigation, StoreError,
};

pub mod yaml_store;
pub use yaml_store::YamlStore;

#[cfg(test)]
mod tests;
