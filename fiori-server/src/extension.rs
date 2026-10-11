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

    /// Zusaetzliche Spalten der Listen-Tabelle der Hauptentitaet `entity`.
    /// Liefert `{ "<Schluessel>": { "template": "...", "header": "...", "position": {...} } }`.
    fn list_columns(&self, _entity: &str) -> Option<Value> {
        None
    }
}

pub type Extensions = Vec<Arc<dyn Extension>>;

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
