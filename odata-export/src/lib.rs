//! Gemeinsame Teile der OData-Werkzeuge: HTTP-Client und `$metadata`-Modell.
//!
//! Die Binaries `odata-export` (Daten lesen) und `odata-import` (Daten schreiben)
//! nutzen dieselben Regeln wie `scripts/odata-export.py` und `scripts/odata-import.py`.

pub mod http;
pub mod model;

/// Entwurfsfelder: beim Export verworfen, beim Import nie gesendet.
pub const DRAFT_PROPERTIES: [&str; 7] = [
    "IsActiveEntity",
    "HasActiveEntity",
    "HasDraftEntity",
    "DraftAdministrativeData",
    "DraftAdministrativeData_ID",
    "DraftMessages",
    "SiblingEntity",
];

/// Formatversion des Exportverzeichnisses (`manifest.json`).
pub const FORMAT_VERSION: u64 = 1;
