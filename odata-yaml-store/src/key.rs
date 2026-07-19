use odata::path::PathSegment;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::hash::Hash;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntityKey {
    pub id: Uuid,
    pub is_active_entity: bool,
}

impl EntityKey {
    pub fn new(id: Uuid, is_active_entity: bool) -> Self {
        Self {
            id,
            is_active_entity,
        }
    }

    pub fn storage_id(&self) -> String {
        format!("{:}", self.id)
    }

    pub fn sibling(&self) -> EntityKey {
        EntityKey {
            id: self.id,
            is_active_entity: !self.is_active_entity,
        }
    }

    pub fn new_from_json(value: &Value) -> Option<EntityKey> {
        let id = value.get("ID").and_then(|x| x.as_str());
        let is_active = value.get("IsActiveEntity").and_then(|x| x.as_bool());
        match (id, is_active) {
            (None, None) => None,
            (None, Some(_)) => None,
            (Some(id), None) => Uuid::parse_str(&id)
                .ok()
                .and_then(|x| Some(EntityKey::new(x, true))),
            (Some(id), Some(a)) => Uuid::parse_str(&id)
                .ok()
                .and_then(|x| Some(EntityKey::new(x, a))),
        }
    }
}

impl From<&PathSegment> for EntityKey {
    fn from(value: &PathSegment) -> Self {
        let k = value
            .keys
            .iter()
            .find(|x| x.name == Some("ID".into()))
            .and_then(|x| Uuid::parse_str(&x.value.to_string_value()).ok());
        EntityKey::new(k.unwrap(), true)
    }
}
/// Referenz auf die uebergeordnete Entity bei Compositions (z.B. Header -> Items).
#[derive(Debug, Clone)]
pub struct ParentKey {
    pub set_name: String,
    pub key: EntityKey,
    pub navigation_property: String,
}
