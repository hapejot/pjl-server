use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

/// Fachlicher Schluessel einer Entity, inkl. RAP-Draft-Dimension.
///
/// `fields` ist eine BTreeMap, damit die Reihenfolge der Key-Felder
/// deterministisch ist (wichtig fuer den Hash in `storage_id`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntityKey {
    pub fields: BTreeMap<String, String>,
    pub is_active_entity: bool,
}

impl EntityKey {
    pub fn new(
        fields: impl IntoIterator<Item = (String, String)>,
        is_active_entity: bool,
    ) -> Self {
        Self {
            fields: fields.into_iter().collect(),
            is_active_entity,
        }
    }

    /// Deterministischer, dateisystemsicherer Name fuer diesen Key.
    /// Composite Keys / GUIDs koennen Sonderzeichen enthalten, daher wird gehasht.
    pub fn storage_id(&self) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Liefert den gegenteiligen Key (aktiv <-> Draft) mit denselben fachlichen Feldern.
    pub fn sibling(&self) -> EntityKey {
        EntityKey {
            fields: self.fields.clone(),
            is_active_entity: !self.is_active_entity,
        }
    }
}

/// Referenz auf die uebergeordnete Entity bei Compositions (z.B. Header -> Items).
#[derive(Debug, Clone)]
pub struct ParentKey {
    pub set_name: String,
    pub key: EntityKey,
    pub navigation_property: String,
}
