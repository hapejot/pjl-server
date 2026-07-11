use std::fmt;

#[derive(Debug)]
pub enum StoreError {
    NotFound(String),
    Io(std::io::Error),
    Serialize(String),
    Deserialize(String),
    Metadata(String),
    Conflict(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::NotFound(s) => write!(f, "Nicht gefunden: {s}"),
            StoreError::Io(e) => write!(f, "IO-Fehler: {e}"),
            StoreError::Serialize(s) => write!(f, "Serialisierungsfehler: {s}"),
            StoreError::Deserialize(s) => write!(f, "Deserialisierungsfehler: {s}"),
            StoreError::Metadata(s) => write!(f, "Metadaten-Fehler: {s}"),
            StoreError::Conflict(s) => write!(f, "Konflikt: {s}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}
