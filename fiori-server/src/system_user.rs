//! Benutzerverwaltung zur Laufzeit.
//!
//! - Der Akteur einer Anfrage ist der Header-User (siehe [`crate::auth`]) oder,
//!   wenn keiner vorhanden ist, der Default User (`none@localhost`).
//! - Unbekannte Header-User werden beim ersten Zugriff als `SystemUser` angelegt.
//! - Inaktive Benutzer werden mit 403 abgewiesen. Der Default User ist immer aktiv.
//! - Beim Anlegen werden `Owner` (falls leer), `CreatedBy`, `CreatedAt` gesetzt;
//!   beim Aendern `ChangedBy`, `ChangedAt`. Diese Felder kann der Client nicht
//!   selbst vorgeben.
//! - Benutzer werden nicht geloescht. Der Default User laesst sich nicht
//!   deaktivieren oder umbenennen.

use std::collections::HashSet;

use chrono::{SecondsFormat, Utc};
use data_store::{DataRecord, DraftDataStore, EntityKey, StoreError};
use odata::query::ODataQuery;
use serde_json::{json, Value};
use spec::system_user::{
    self as su, default_user_id, CHANGED_AT, CHANGED_BY, CREATED_AT, CREATED_BY, OWNER,
    SYSTEM_USER_SET,
};
use uuid::Uuid;

use crate::hook::{Extender, Hook, HookError, Operation};

tokio::task_local! {
    /// Username des Header-Users der laufenden Anfrage. `None`, wenn keiner
    /// gesetzt ist oder ausserhalb einer Anfrage gearbeitet wird.
    pub static CURRENT_USER: Option<String>;
}

fn current_user() -> Option<String> {
    CURRENT_USER.try_with(|u| u.clone()).ok().flatten()
}

/// Liefert den Benutzer aus einem Datensatz von `SystemUser`.
fn username_of(record: &DataRecord) -> Option<String> {
    record.get(su::USERNAME)?.as_str().map(str::to_string)
}

/// Sucht einen Benutzer ueber den Username (ohne Hooks, direkt am Store).
async fn find_user(raw: &dyn DraftDataStore, username: &str) -> Result<Option<DataRecord>, StoreError> {
    let all = raw.get_collection(SYSTEM_USER_SET, &ODataQuery::new()).await?;
    Ok(all
        .into_iter()
        .find(|r| username_of(r).as_deref() == Some(username)))
}

/// Legt einen aktivierten Benutzer an (Entwurf anlegen, sofort aktivieren).
async fn create_user(
    raw: &dyn DraftDataStore,
    username: &str,
    name: Option<&str>,
    active: bool,
    id: Option<Uuid>,
) -> Result<Uuid, StoreError> {
    let mut data = json!({ su::USERNAME: username, su::ACTIVE: active });
    if let Some(name) = name {
        data[su::NAME] = json!(name);
    }
    if let Some(id) = id {
        data["ID"] = json!(id.to_string());
    }
    let created = raw.create_entity(SYSTEM_USER_SET, &data).await?;
    let id = created.id();
    raw.draft_activate(SYSTEM_USER_SET, &EntityKey::new(id, false))
        .await?;
    Ok(id)
}

/// Ermittelt den Akteur der Anfrage und prueft, ob er arbeiten darf.
pub async fn resolve_actor(raw: &dyn DraftDataStore) -> Result<Uuid, HookError> {
    let Some(username) = current_user() else {
        return Ok(default_user_id());
    };
    match find_user(raw, &username).await? {
        Some(record) => {
            if record.get(su::ACTIVE) != Some(json!(true)) {
                return Err(HookError::Store(StoreError::Forbidden(format!(
                    "Benutzer '{username}' ist inaktiv"
                ))));
            }
            Ok(record.id())
        }
        None => Ok(create_user(raw, &username, None, true, None).await?),
    }
}

/// Stellt sicher, dass der Default User mit fester ID vorhanden und aktiv ist.
/// Laeuft bei jedem Start; Name und Aktiv-Flag werden zurueckgesetzt.
pub async fn ensure_default_user(raw: &dyn DraftDataStore) -> anyhow::Result<()> {
    let id = default_user_id();
    match find_user(raw, su::DEFAULT_USERNAME).await? {
        None => {
            create_user(
                raw,
                su::DEFAULT_USERNAME,
                Some(su::DEFAULT_USER_NAME),
                true,
                Some(id),
            )
            .await?;
        }
        Some(record) => {
            let key = EntityKey::new(record.id(), true);
            let wanted = json!({ su::NAME: su::DEFAULT_USER_NAME, su::ACTIVE: true });
            raw.patch_entity(SYSTEM_USER_SET, &key, &wanted).await?;
        }
    }
    Ok(())
}

/// Hook: Zugriffspruefung, Benutzerregeln und Audit-Felder.
///
/// Muss innerhalb aller anderen Hooks liegen, damit sein Store-Zugriff
/// (ueber [`Extender::raw`]) nicht rekursiv durch ihn selbst laeuft.
pub struct SystemUserHook {
    /// Sets, die Audit-Felder tragen (alle Entitaeten des Dictionarys).
    audited: HashSet<String>,
}

impl SystemUserHook {
    pub fn new(audited: HashSet<String>) -> Self {
        Self { audited }
    }

    async fn guard(
        &self,
        store: &Extender,
        set: &str,
        op: Operation,
        key: Option<&EntityKey>,
        value: &mut Value,
    ) -> Result<(), HookError> {
        let raw = store.raw().as_ref();
        let actor = resolve_actor(raw).await?;

        if set == SYSTEM_USER_SET {
            check_user(raw, op, key, value).await?;
        }
        if !self.audited.contains(set) {
            return Ok(());
        }
        match op {
            Operation::Create => audit_create(value, actor),
            Operation::Patch => audit_patch(value, actor),
            _ => {}
        }
        Ok(())
    }
}

/// Regeln fuer `SystemUser` selbst.
async fn check_user(
    raw: &dyn DraftDataStore,
    op: Operation,
    key: Option<&EntityKey>,
    value: &Value,
) -> Result<(), HookError> {
    match op {
        Operation::Delete => Err(HookError::Store(StoreError::Forbidden(
            "Benutzer koennen nicht geloescht werden".into(),
        ))),
        Operation::Create => {
            let name = value.get(su::USERNAME).and_then(Value::as_str).unwrap_or("").trim();
            if name.is_empty() {
                return Err(HookError::Hook("Username fehlt".into()));
            }
            ensure_unique(raw, name, None).await
        }
        Operation::Patch => {
            let Some(key) = key else { return Ok(()) };
            if key.id == default_user_id() {
                if value.get(su::ACTIVE) == Some(&json!(false)) {
                    return Err(HookError::Store(StoreError::Forbidden(
                        "Der Default User kann nicht deaktiviert werden".into(),
                    )));
                }
                if value.get(su::USERNAME).is_some_and(|u| u.as_str() != Some(su::DEFAULT_USERNAME)) {
                    return Err(HookError::Store(StoreError::Forbidden(
                        "Der Default User kann nicht umbenannt werden".into(),
                    )));
                }
            }
            match value.get(su::USERNAME).and_then(Value::as_str) {
                Some(name) => ensure_unique(raw, name.trim(), Some(key.id)).await,
                None => Ok(()),
            }
        }
        _ => Ok(()),
    }
}

async fn ensure_unique(
    raw: &dyn DraftDataStore,
    username: &str,
    except: Option<Uuid>,
) -> Result<(), HookError> {
    match find_user(raw, username).await? {
        Some(rec) if Some(rec.id()) != except => Err(HookError::Store(StoreError::Conflict(
            format!("Benutzername '{username}' ist bereits vergeben"),
        ))),
        _ => Ok(()),
    }
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn fk(name: &str) -> String {
    su::fk_column(name)
}

/// Beim Anlegen: Owner (falls leer) = Akteur; Ersteller und Zeit setzen.
/// Vom Client mitgegebene Aenderungsfelder werden verworfen.
fn audit_create(value: &mut Value, actor: Uuid) {
    let Some(obj) = value.as_object_mut() else { return };
    let owner_empty = match obj.get(&fk(OWNER)) {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.is_empty(),
        _ => false,
    };
    if owner_empty {
        obj.insert(fk(OWNER), json!(actor.to_string()));
    }
    obj.insert(fk(CREATED_BY), json!(actor.to_string()));
    obj.insert(CREATED_AT.into(), json!(now()));
    obj.remove(&fk(CHANGED_BY));
    obj.remove(CHANGED_AT);
}

/// Beim Aendern: Aenderer und Zeit setzen. Ersteller bleibt unveraendert.
fn audit_patch(value: &mut Value, actor: Uuid) {
    let Some(obj) = value.as_object_mut() else { return };
    obj.remove(&fk(CREATED_BY));
    obj.remove(CREATED_AT);
    obj.insert(fk(CHANGED_BY), json!(actor.to_string()));
    obj.insert(CHANGED_AT.into(), json!(now()));
}

#[async_trait::async_trait]
impl Hook for SystemUserHook {
    async fn before(
        &self,
        store: &Extender,
        set_name: &str,
        op: Operation,
        value: &mut Value,
    ) -> Result<(), HookError> {
        self.guard(store, set_name, op, None, value).await
    }

    async fn before_patch(
        &self,
        store: &Extender,
        set_name: &str,
        key: &EntityKey,
        patch: &mut Value,
    ) -> Result<(), HookError> {
        self.guard(store, set_name, Operation::Patch, Some(key), patch)
            .await
    }

    async fn before_read(
        &self,
        store: &Extender,
        set_name: &str,
        key: Option<&EntityKey>,
        query: &mut ODataQuery,
    ) -> Result<(), HookError> {
        let _ = (set_name, key, query);
        resolve_actor(store.raw().as_ref()).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_sets_owner_and_created_fields_and_drops_client_changes() {
        let actor = Uuid::new_v4();
        let mut v = json!({"name": "x", "ChangedBy_ID": "evil", "ChangedAt": "x"});
        audit_create(&mut v, actor);
        assert_eq!(v["Owner_ID"], json!(actor.to_string()));
        assert_eq!(v["CreatedBy_ID"], json!(actor.to_string()));
        assert!(v.get("CreatedAt").is_some());
        assert!(v.get("ChangedBy_ID").is_none());
        assert!(v.get("ChangedAt").is_none());
    }

    #[test]
    fn create_keeps_explicit_owner() {
        let actor = Uuid::new_v4();
        let other = Uuid::new_v4();
        let mut v = json!({"Owner_ID": other.to_string()});
        audit_create(&mut v, actor);
        assert_eq!(v["Owner_ID"], json!(other.to_string()));
        assert_eq!(v["CreatedBy_ID"], json!(actor.to_string()));
    }

    #[test]
    fn patch_sets_changed_and_protects_created() {
        let actor = Uuid::new_v4();
        let mut v = json!({"name": "y", "CreatedBy_ID": "evil", "CreatedAt": "x"});
        audit_patch(&mut v, actor);
        assert_eq!(v["ChangedBy_ID"], json!(actor.to_string()));
        assert!(v.get("ChangedAt").is_some());
        assert!(v.get("CreatedBy_ID").is_none());
        assert!(v.get("CreatedAt").is_none());
        assert_eq!(v["name"], json!("y"));
    }

    #[tokio::test]
    async fn current_user_defaults_outside_request() {
        assert_eq!(current_user(), None);
        let seen = CURRENT_USER
            .scope(Some("peter".into()), async { current_user() })
            .await;
        assert_eq!(seen.as_deref(), Some("peter"));
    }
}
