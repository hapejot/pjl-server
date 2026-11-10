use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
/// Logisches Paket zur Gruppierung von Entitaeten und Apps.
pub struct Package {
    name: String,
}

impl Package {
    /// Erzeugt ein Paket mit dem gegebenen Namen.
    pub fn new(name: &str) -> Self {
        Self { name: name.into() }
    }

    /// Name des Pakets.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }
}
