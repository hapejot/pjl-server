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

pub const USERNAME: &str = "Username";
pub const NAME: &str = "Name";
pub const ACTIVE: &str = "Active";

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
