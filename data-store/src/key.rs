use odata::path::PathSegment;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::hash::Hash;
use uuid::Uuid;

/// Einzelner Primaerschluessel einer Entitaet: UUID (Standard `ID`) oder Text
/// (eigener Schluessel, z. B. ein Code). Immer genau ein Wert, nie zusammengesetzt.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeyValue {
    Uuid(Uuid),
    Text(String),
}

impl KeyValue {
    /// Parst einen Schluesselwert: gueltige UUID wird `Uuid`, alles andere `Text`.
    pub fn parse(s: &str) -> Self {
        match Uuid::parse_str(s) {
            Ok(u) => KeyValue::Uuid(u),
            Err(_) => KeyValue::Text(s.to_string()),
        }
    }

    pub fn as_uuid(&self) -> Option<Uuid> {
        match self {
            KeyValue::Uuid(u) => Some(*u),
            KeyValue::Text(_) => None,
        }
    }
}

impl From<Uuid> for KeyValue {
    fn from(value: Uuid) -> Self {
        KeyValue::Uuid(value)
    }
}

impl From<String> for KeyValue {
    fn from(value: String) -> Self {
        KeyValue::Text(value)
    }
}

impl From<&str> for KeyValue {
    fn from(value: &str) -> Self {
        KeyValue::Text(value.to_string())
    }
}

impl fmt::Display for KeyValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyValue::Uuid(u) => write!(f, "{u}"),
            KeyValue::Text(t) => f.write_str(t),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntityKey {
    pub id: KeyValue,
    pub is_active_entity: bool,
}

impl EntityKey {
    pub fn new(id: impl Into<KeyValue>, is_active_entity: bool) -> Self {
        Self {
            id: id.into(),
            is_active_entity,
        }
    }

    pub fn new_active(id: impl Into<String>) -> Self {
        Self {
            id: KeyValue::parse(&id.into()),
            is_active_entity: true,
        }
    }

    pub fn storage_id(&self) -> String {
        self.id.to_string()
    }

    pub fn sibling(&self) -> EntityKey {
        EntityKey {
            id: self.id.clone(),
            is_active_entity: !self.is_active_entity,
        }
    }

    /// Liest den Schluessel `ID` (Standard) aus einem JSON-Objekt.
    pub fn new_from_json(value: &Value) -> Option<EntityKey> {
        Self::new_from_json_with(value, "ID")
    }

    /// Liest den Primaerschluessel `pk` und optional `IsActiveEntity` aus einem JSON-Objekt.
    pub fn new_from_json_with(value: &Value, pk: &str) -> Option<EntityKey> {
        let id = match value.get(pk)? {
            Value::String(s) => KeyValue::parse(s),
            Value::Number(n) => KeyValue::Text(n.to_string()),
            _ => return None,
        };
        let is_active = value.get("IsActiveEntity").and_then(|x| x.as_bool());
        Some(EntityKey::new(id, is_active.unwrap_or(true)))
    }
}

impl From<&PathSegment> for EntityKey {
    /// Standardschluessel `ID`. Fuer eigene Schluessel `EntityKey::from_segment_with`.
    fn from(value: &PathSegment) -> Self {
        Self::from_segment_with(value, "ID")
    }
}

impl EntityKey {
    /// Liest den Primaerschluessel `pk` und `IsActiveEntity` aus einem Pfadsegment.
    pub fn from_segment_with(value: &PathSegment, pk: &str) -> Self {
        let k = value
            .keys
            .iter()
            .find(|x| x.name.as_deref() == Some(pk))
            .map(|x| KeyValue::parse(&x.value.to_string_value()))
            .expect("Schluessel im Pfadsegment");
        let is_active = value
            .keys
            .iter()
            .find(|x| x.name.as_deref() == Some("IsActiveEntity"))
            .and_then(|x| x.value.as_bool());
        EntityKey::new(k, is_active.unwrap_or(true))
    }
}

impl KeyValue {
    /// Filter `field eq <Schluessel>`: UUID als GUID-Literal, Text als Zeichenkette.
    pub fn equals_filter(&self, field: &str) -> odata::query::Expr {
        match self {
            KeyValue::Uuid(u) => odata::query::Expr::equals_guid(u.to_string(), field),
            KeyValue::Text(t) => odata::query::Expr::equals_value(
                &serde_json::Value::String(t.clone()),
                field,
            ),
        }
    }
}
