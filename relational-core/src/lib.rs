//! Gemeinsamer Kern der relationalen Backends.
//!
//! Hier steht alles, was SQL **erzeugt**, aber nichts **ausfuehrt**: die
//! Uebersetzung von OData nach SQL, die DDL- und Migrationsplanung und die
//! Stellen, an denen sich die Dialekte unterscheiden. Kein Treiber, keine
//! Verbindung - entsprechend ohne laufende Datenbank testbar.
//!
//! Die treibernahen Teile bleiben bei den Backends:
//!
//! * die Implementierung von [`catalog::Catalog`] (Systemkatalog),
//! * die Umrechnung von [`data_store::SqlParam`] auf die Typen des Treibers,
//! * das Absetzen der Anweisungen.

pub mod catalog;
pub mod ddl;
pub mod dialect;
pub mod sql;

pub use catalog::Catalog;
pub use ddl::quote;
pub use dialect::Dialect;
pub use sql::Sql;
