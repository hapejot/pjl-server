//! Datenverzeichnis (DDIC) mit Entitäten,
//! Beziehungen, Apps, Komponenten und Festwertlisten.
//!
//! Die Typen werden über die Module angesprochen (z. B.
//! [`data_dictionary::DataDictionary`]); häufig gebrauchte Typen sind zusätzlich
//! an der Wurzel re-exportiert. Die Serialisierung (`ddic.json`) ist Teil des
//! Vertrags und wird über `serde` abgebildet.
//!
//! # Beispiel
//! 
//! Validierung und Verwaltung von Entitäten, Beziehungen, Apps, Komponenten und Festwertlisten.
//! 
//! ```
//! use spec::{DataDictionary, EntitySpec, FieldSpec};
//!
//! let mut ddic = DataDictionary::empty();
//! ddic.add_entity(
//!     EntitySpec::new("Orders", "core", "Orders")
//!         .field(FieldSpec::string("title", "Titel", 40)),
//! );
//! assert!(ddic.entity_names().any(|name| name == "Orders"));
//! ```

#![warn(missing_docs)]

/// Anwendungen (Fiori-Einstiege) und ihr `ui`-Block.
pub mod app;
pub mod component;
/// Datenwoerterbuch (DDIC), Wurzel aller Spezifikationen.
pub mod data_dictionary;
/// Entitaeten, Felder, Listenansichten und Object-Page-Einstellungen.
pub mod entity_spec;
/// Pakete zur Gruppierung von Entitaeten und Apps.
pub mod package;
/// Beziehungen zwischen Entitaeten.
pub mod relationship;
pub mod system_user;
pub mod value_list;

pub use app::{App, AppUi, ControllerExtension};
pub use component::{ComponentKind, ComponentSpec, ComponentUse, TimeUnit};
pub use data_dictionary::DataDictionary;
pub use entity_spec::{
    AtomValueList, ConstantFilter, DisplayFormat, EntitySpec, EntityUi, FacetSectionSpec,
    FacetSpec, FieldSpec, FieldUi, FilterDefault, ID_SUFFIX, ListColumn, ListView, MeasureKind,
    Priority, TableFacetSpec, UiListColumn, UiListTemplate, UiSection, ValueListFilter,
};
pub use package::Package;
pub use relationship::{Condition, ConditionalRef, Relationship, Side};
pub use value_list::{ValueEntry, ValueList};
