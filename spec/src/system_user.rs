//! Benutzerverwaltung: Entitaet `SystemUser` und die Audit-Felder, die jede
//! Entitaet beim Laden des Dictionarys bekommt.
//!
//! Die Ergaenzung passiert nur im Speicher nach dem Einlesen der `ddic.json`;
//! die Datei wird nicht veraendert.

use uuid::Uuid;

use crate::FieldSpec;

/// Name der Benutzer-Entitaet.
pub const SYSTEM_USER: &str = "SystemUser";
/// Name des EntitySets der Benutzer.
pub const SYSTEM_USER_SET: &str = "SystemUserSet";
/// Paket der Benutzer-Entitaet.
pub const PACKAGE: &str = "core";

/// Feld fuer den Benutzernamen.
pub const USERNAME: &str = "name";
/// Feld fuer den vollen Namen.
pub const NAME: &str = "fullname";
/// Feld fuer den Aktiv-Status.
pub const ACTIVE: &str = "active";

/// Profil- und Einstellungsfelder aus dem Benutzermenue (UI5 `sap/ushell/User`).
/// Heute nur lesend (`/flp/me`); Bearbeitung folgt spaeter.
/// Namen entsprechen den UI5-Schluesseln, damit die Zuordnung 1:1 ist.
pub const EMAIL: &str = "email";
/// Design (UI5-Schluessel `theme`).
pub const THEME: &str = "theme";
/// Inhaltsdichte (UI5-Schluessel).
pub const CONTENT_DENSITY: &str = "contentDensity";
/// Barrierefreiheit (UI5-Schluessel).
pub const ACCESSIBILITY: &str = "accessibility";
/// Dunkelmodus erkennen (UI5-Schluessel).
pub const DETECT_DARK_MODE: &str = "detectDarkMode";
/// Sprache (UI5-Schluessel).
pub const LANGUAGE: &str = "language";
/// Zeitzone (UI5-Schluessel).
pub const TIME_ZONE: &str = "timeZone";
/// Kalenderwochen-Nummerierung (UI5-Schluessel).
pub const CALENDAR_WEEK_NUMBERING: &str = "calendarWeekNumbering";
/// Nutzeraktivitaeten verfolgen (UI5-Schluessel).
pub const TRACK_USER_ACTIVITIES: &str = "trackUserActivities";
/// Startseite anzeigen (UI5-Schluessel).
pub const SHOW_MY_HOME: &str = "showMyHome";
/// Lesezeichen importieren (UI5-Schluessel).
pub const IMPORT_BOOKMARKS: &str = "importBookmarks";
/// Einwilligung zu Bildern (UI5-Schluessel).
pub const IMAGE_CONSENT: &str = "imageConsent";
/// Nutzungsanalyse (UI5-Schluessel).
pub const TRACK_USAGE_ANALYTICS: &str = "trackUsageAnalytics";

/// Alle Profil- und Einstellungsfelder (ohne `name`, `fullname`, `active`).
pub const PROFILE_FIELDS: &[&str] = &[
    EMAIL,
    THEME,
    CONTENT_DENSITY,
    ACCESSIBILITY,
    DETECT_DARK_MODE,
    LANGUAGE,
    TIME_ZONE,
    CALENDAR_WEEK_NUMBERING,
    TRACK_USER_ACTIVITIES,
    SHOW_MY_HOME,
    IMPORT_BOOKMARKS,
    IMAGE_CONSENT,
    TRACK_USAGE_ANALYTICS,
];

/// Audit-Feld: Eigentuemer.
pub const OWNER: &str = "Owner";
/// Audit-Feld: Angelegt von.
pub const CREATED_BY: &str = "CreatedBy";
/// Audit-Feld: Geaendert von.
pub const CHANGED_BY: &str = "ChangedBy";
/// Audit-Feld: Angelegt am.
pub const CREATED_AT: &str = "CreatedAt";
/// Audit-Feld: Geaendert am.
pub const CHANGED_AT: &str = "ChangedAt";

/// Username des Default Users. Wird verwendet, wenn kein Benutzer bekannt ist.
pub const DEFAULT_USERNAME: &str = "none@localhost";
/// Anzeigename des Standardbenutzers.
pub const DEFAULT_USER_NAME: &str = "Default User";

/// Namespace fuer die UUIDv5 der Benutzer. Ableitung aus dem Namen `SystemUser`
/// unter `NAMESPACE_DNS`. Darf nicht geaendert werden, sonst aendert sich die
/// ID des Default Users.
pub fn namespace() -> Uuid {
    Uuid::new_v5(&Uuid::NAMESPACE_DNS, SYSTEM_USER.as_bytes())
}

/// Feste ID des Default Users: UUIDv5(namespace, "none@localhost").
pub fn default_user_id() -> Uuid {
    Uuid::new_v5(&namespace(), DEFAULT_USERNAME.as_bytes())
}

/// Audit-Felder, die jede Entitaet bekommt: Owner, Ersteller, Aenderer
/// (Verweise auf `SystemUser`) und die Zeitstempel. Schreiben tut nur der Server
/// (siehe `fiori-server/src/system_user.rs`); Client-Werte werden dort verworfen.
pub fn audit_fields() -> Vec<FieldSpec> {
    vec![
        FieldSpec::Lookup {
            name: OWNER.into(),
            entity: SYSTEM_USER.into(),
            collection: false,
            owned: false,
            label: Some("Owner".into()),
        },
        FieldSpec::Lookup {
            name: CREATED_BY.into(),
            entity: SYSTEM_USER.into(),
            collection: false,
            owned: false,
            label: Some("Created By".into()),
        },
        FieldSpec::Lookup {
            name: CHANGED_BY.into(),
            entity: SYSTEM_USER.into(),
            collection: false,
            owned: false,
            label: Some("Changed By".into()),
        },
        FieldSpec::atom(CREATED_AT, "Created At", "Edm.DateTimeOffset"),
        FieldSpec::atom(CHANGED_AT, "Changed At", "Edm.DateTimeOffset"),
    ]
}

/// Ob `name` in der Oberflaeche nicht schreibbar ist. Alle Audit-Felder ausser
/// `Owner`, der in der GUI setzbar bleibt (siehe `audit_fields`).
pub fn is_read_only_audit_field(name: &str) -> bool {
    matches!(name, CREATED_BY | CHANGED_BY | CREATED_AT | CHANGED_AT)
}

/// Name des Fremdschluessel-Feldes (`{name}_ID`) fuer eine Navigation.
pub fn fk_column(navigation: &str) -> String {
    format!("{navigation}_ID")
}

#[cfg(test)]
mod tests;
