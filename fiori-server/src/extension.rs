//! Erweiterungspunkte des Fiori-Servers.
//!
//! Der Server kennt keine konkreten Fachmodelle. Ein Binary, das ihn benutzt,
//! reicht Erweiterungen ein, die ihn um Routen und UI-Beitraege ergaenzen:
//!
//! * zusaetzliche HTTP-Routen (z. B. Proxy, Fragmente),
//! * UI5-Modulpfade fuer eigene Fragmente/Controller,
//! * Beitraege zu `content` der Object Page einer Hauptentitaet
//!   (Custom Sections, ausgeblendete Standard-Facets).
//!
//! Abhaengigkeit nur in eine Richtung: Erweiterungen kennen den Server,
//! der Server kennt keine Erweiterung.

use std::sync::Arc;

use axum::Router;
use data_store::EntityKey;
use serde_json::{json, Value};

use crate::AppState;

pub trait Extension: Send + Sync {
    /// Zusaetzliche Routen. Bekommt den Server-Zustand (Store, Konfiguration),
    /// liefert einen fertigen Router ohne offenen State.
    fn routes(&self, _state: AppState) -> Router {
        Router::new()
    }

    /// UI5-Modulpfade als `(Namespace, URL)`, z. B. `("video.ext", "/ext/video")`.
    /// Werden im generierten `Component.js` registriert.
    fn ui5_paths(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Beitrag zu `content` der Object Page der Hauptentitaet `entity`.
    /// Erwartet das Format `{"body": {"sections": {...}}}`.
    fn object_page_content(&self, _entity: &str) -> Option<Value> {
        None
    }

    /// Zusaetzliche Ansichten der ListReport-Seite (View-Leiste) der Hauptentitaet
    /// `entity`, z. B. eine Custom-View mit eigenem Fragment:
    /// `{"key": "...", "label": "...", "template": "ns.fragment.Name"}`.
    fn list_views(&self, _entity: &str) -> Vec<Value> {
        Vec::new()
    }

    /// Zusaetzliche Spalten der Listen-Tabelle der Hauptentitaet `entity`.
    /// Liefert `{ "<Schluessel>": { "template": "...", "header": "...", "position": {...} } }`.
    fn list_columns(&self, _entity: &str) -> Option<Value> {
        None
    }

    /// Gebundene Aktionen auf Entity Sets (siehe [`BoundAction`]).
    fn actions(&self) -> Vec<BoundAction> {
        Vec::new()
    }

    /// Felder der Hauptentitaet `entity`, die aus dem Formular der Standard-Object-Page
    /// entfallen (z. B. weil eine Erweiterung sie in einem eigenen Abschnitt zeigt).
    fn hidden_fields(&self, _entity: &str) -> Vec<String> {
        Vec::new()
    }

    /// UI5-Module, die mit der Komponente geladen werden, z. B. `video/ext/Format`
    /// (Pfad ueber `ui5_paths` aufgeloest). Globale Formatter brauchen das.
    fn ui5_modules(&self) -> Vec<String> {
        Vec::new()
    }

    /// Controller-Erweiterungen als `(Ziel-Controller, Controller-Name)`, z. B.
    /// `("sap.fe.templates.ObjectPage.ObjectPageController", "video.ext.ObjectPageController")`.
    /// Gehen ins Manifest unter `sap.ui5/extends/extensions/sap.ui.controllerExtensions`.
    fn controller_extensions(&self) -> Vec<(String, String)> {
        Vec::new()
    }
}


pub type Extensions = Vec<Arc<dyn Extension>>;

/// Fehler einer Erweiterungs-Aktion. Wird in `srv` auf einen OData-Fehler abgebildet.
#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    /// Fachlicher Fehler, z. B. "Video hat keinen Agenten".
    #[error("{0}")]
    Failed(String),
    /// Fehler aus dem Store.
    #[error(transparent)]
    Store(#[from] data_store::StoreError),
}

/// Implementierung einer Aktion. `key` ist der Schluessel der gebundenen Instanz.
/// `Value::Null` als Ergebnis wird als `204 No Content` beantwortet.
#[async_trait::async_trait]
pub trait ActionHandler: Send + Sync {
    async fn invoke(&self, state: &AppState, key: &EntityKey) -> Result<Value, ActionError>;
}

/// Gebundene Aktion (`IsBound="true"`) auf dem Entity Set `set`, z. B. `VideoSet`.
/// Aufruf per `POST /Service/<set>(<key>)/Service.<name>`.
#[derive(Clone)]
pub struct BoundAction {
    /// Entity Set, auf dem die Aktion liegt (z. B. `VideoSet`).
    pub set: String,
    /// Name der Aktion ohne Namespace (z. B. `convert`).
    pub name: String,
    /// Entitaetstyp des Rueckgabewerts (z. B. `VideoJob`), `None` ohne Rueckgabe.
    pub returns: Option<String>,
    pub handler: Arc<dyn ActionHandler>,
    /// Button im Header der Object Page; `None` = nur per API aufrufbar.
    pub header: Option<HeaderButton>,
    /// Verfuegbarkeit: die Aktion ist nur verfuegbar, wenn die Eigenschaft den Wert hat.
    /// Wird als `Core.OperationAvailable` am Action-Element deklariert.
    pub available_if: Option<Availability>,
}

/// `<property> == <equals>` an der gebundenen Instanz (`in/<property>` in CSDL).
/// Unterstuetzt werden Boolean- und String-Werte.
#[derive(Clone)]
pub struct Availability {
    pub property: String,
    pub equals: Value,
}

/// Aktion als Button im Object-Page-Header (`UI.Identification`, `DataFieldForAction`).
#[derive(Clone)]
pub struct HeaderButton {
    /// Beschriftung des Buttons.
    pub text: String,
}

/// Alle gebundenen Aktionen aller Erweiterungen.
pub fn bound_actions(extensions: &[Arc<dyn Extension>]) -> Vec<BoundAction> {
    extensions.iter().flat_map(|ext| ext.actions()).collect()
}

/// Gebundene Aktion `name` auf dem Entity Set `set`.
pub fn find_action(extensions: &[Arc<dyn Extension>], set: &str, name: &str) -> Option<BoundAction> {
    bound_actions(extensions)
        .into_iter()
        .find(|a| a.set == set && a.name == name)
}

/// Zusammengefuehrtes `content` der Object Page fuer `entity`, `None` ohne Beitraege.
pub fn object_page_content(extensions: &[Arc<dyn Extension>], entity: &str) -> Option<Value> {
    let mut sections = serde_json::Map::new();
    for ext in extensions {
        if let Some(body_sections) = ext
            .object_page_content(entity)
            .as_ref()
            .and_then(|c| c.pointer("/body/sections"))
            .and_then(|s| s.as_object())
        {
            for (k, v) in body_sections {
                sections.insert(k.clone(), v.clone());
            }
        }
    }
    if sections.is_empty() {
        None
    } else {
        Some(json!({ "body": { "sections": sections } }))
    }
}

/// Zusaetzliche View-Eintraege aller Erweiterungen fuer die ListReport von `entity`.
pub fn list_views(extensions: &[Arc<dyn Extension>], entity: &str) -> Vec<Value> {
    extensions.iter().flat_map(|ext| ext.list_views(entity)).collect()
}

/// Ausgeblendete Formularfelder der Hauptentitaet `entity` (alle Erweiterungen).
pub fn hidden_fields(extensions: &[Arc<dyn Extension>], entity: &str) -> Vec<String> {
    extensions
        .iter()
        .flat_map(|ext| ext.hidden_fields(entity))
        .collect()
}

/// `sap.ui5/extends`-Block des Manifests fuer die Controller-Erweiterungen aller Erweiterungen.
pub fn controller_extensions_manifest(extensions: &[Arc<dyn Extension>]) -> Value {
    let mut map = serde_json::Map::new();
    for ext in extensions {
        for (target, controller) in ext.controller_extensions() {
            map.insert(target, json!({ "controllerName": controller }));
        }
    }
    if map.is_empty() {
        json!({})
    } else {
        json!({ "extensions": { "sap.ui.controllerExtensions": map } })
    }
}

/// UI5-Module aller Erweiterungen, in Registrierungsreihenfolge.
pub fn ui5_modules(extensions: &[Arc<dyn Extension>]) -> Vec<String> {
    extensions.iter().flat_map(|ext| ext.ui5_modules()).collect()
}

/// Zusaetzliche Listenspalten aller Erweiterungen, als `columns`-Objekt der
/// `@com.sap.vocabularies.UI.v1.LineItem`-Konfiguration.
pub fn list_columns(extensions: &[Arc<dyn Extension>], entity: &str) -> Option<Value> {
    let mut columns = serde_json::Map::new();
    for ext in extensions {
        if let Some(Value::Object(cols)) = ext.list_columns(entity) {
            for (k, v) in cols {
                columns.insert(k, v);
            }
        }
    }
    if columns.is_empty() {
        None
    } else {
        Some(Value::Object(columns))
    }
}
