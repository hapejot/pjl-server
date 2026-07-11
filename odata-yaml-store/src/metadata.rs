use crate::error::StoreError;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct NavigationDef {
    pub target_set: String,
    /// Feldname im Kind-Record, der auf den Parent-Key verweist, z.B. "OrderID"
    pub foreign_key_field: String,
}

#[derive(Default)]
pub struct EntityMetadata {
    key_fields: HashMap<String, Vec<String>>,
    navigations: HashMap<String, HashMap<String, NavigationDef>>,
}

impl EntityMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registriert die Key-Felder eines Entity-Sets, in kanonischer Reihenfolge.
    pub fn register_key(&mut self, set_name: &str, fields: &[&str]) {
        self.key_fields.insert(
            set_name.to_string(),
            fields.iter().map(|f| f.to_string()).collect(),
        );
    }

    /// Registriert eine Navigation Property (fuer $expand und Composition-Scoping).
    pub fn register_navigation(&mut self, set_name: &str, nav_property: &str, def: NavigationDef) {
        self.navigations
            .entry(set_name.to_string())
            .or_default()
            .insert(nav_property.to_string(), def);
    }

    pub fn key_fields(&self, set_name: &str) -> Result<&[String], StoreError> {
        self.key_fields
            .get(set_name)
            .map(|v| v.as_slice())
            .ok_or_else(|| StoreError::Metadata(format!("Keine Key-Felder fuer Set '{set_name}' registriert")))
    }

    pub fn lookup_navigation(&self, set_name: &str, nav_property: &str) -> Result<&NavigationDef, StoreError> {
        self.navigations
            .get(set_name)
            .and_then(|m| m.get(nav_property))
            .ok_or_else(|| {
                StoreError::Metadata(format!("Unbekannte Navigation {set_name}/{nav_property}"))
            })
    }
}
