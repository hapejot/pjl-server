//! Benutzerverwaltung: Entitaet `SystemUser` und die Audit-Felder, die jede
//! Entitaet beim Laden des Dictionarys bekommt.
//!
//! Die Ergaenzung passiert nur im Speicher nach dem Einlesen der `ddic.json`;
//! die Datei wird nicht veraendert.

use uuid::Uuid;

use crate::FieldSpec;

pub const SYSTEM_USER: &str = "SystemUser";
pub const SYSTEM_USER_SET: &str = "SystemUserSet";
pub const PACKAGE: &str = "core";

pub const USERNAME: &str = "name";
pub const NAME: &str = "fullname";
pub const ACTIVE: &str = "active";

/// Profil- und Einstellungsfelder aus dem Benutzermenue (UI5 `sap/ushell/User`).
/// Heute nur lesend (`/flp/me`); Bearbeitung folgt spaeter.
/// Namen entsprechen den UI5-Schluesseln, damit die Zuordnung 1:1 ist.
pub const EMAIL: &str = "email";
pub const THEME: &str = "theme";
pub const CONTENT_DENSITY: &str = "contentDensity";
pub const ACCESSIBILITY: &str = "accessibility";
pub const DETECT_DARK_MODE: &str = "detectDarkMode";
pub const LANGUAGE: &str = "language";
pub const TIME_ZONE: &str = "timeZone";
pub const CALENDAR_WEEK_NUMBERING: &str = "calendarWeekNumbering";
pub const TRACK_USER_ACTIVITIES: &str = "trackUserActivities";
pub const SHOW_MY_HOME: &str = "showMyHome";
pub const IMPORT_BOOKMARKS: &str = "importBookmarks";
pub const IMAGE_CONSENT: &str = "imageConsent";
pub const TRACK_USAGE_ANALYTICS: &str = "trackUsageAnalytics";

/// Alle Profil- und Einstellungsfelder (ohne `name`, `fullname`, `active`).
pub const PROFILE_FIELDS: &[&str] = &[
    EMAIL, THEME, CONTENT_DENSITY, ACCESSIBILITY, DETECT_DARK_MODE, LANGUAGE, TIME_ZONE,
    CALENDAR_WEEK_NUMBERING, TRACK_USER_ACTIVITIES, SHOW_MY_HOME, IMPORT_BOOKMARKS,
    IMAGE_CONSENT, TRACK_USAGE_ANALYTICS,
];

pub const OWNER: &str = "Owner";
pub const CREATED_BY: &str = "CreatedBy";
pub const CHANGED_BY: &str = "ChangedBy";
pub const CREATED_AT: &str = "CreatedAt";
pub const CHANGED_AT: &str = "ChangedAt";

/// Username des Default Users. Wird verwendet, wenn kein Benutzer bekannt ist.
pub const DEFAULT_USERNAME: &str = "none@localhost";
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
mod tests {
    use super::*;

    #[test]
    fn default_user_id_is_stable() {
        // Festwert: aendert sich nur, wenn Namespace oder Username geaendert werden.
        assert_eq!(
            default_user_id().to_string(),
            Uuid::new_v5(&Uuid::new_v5(&Uuid::NAMESPACE_DNS, b"SystemUser"), b"none@localhost")
                .to_string()
        );
        assert_eq!(default_user_id(), default_user_id());
    }

    #[test]
    fn fk_column_suffix() {
        assert_eq!(fk_column("Owner"), "Owner_ID");
    }
}
