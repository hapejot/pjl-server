use std::{backtrace::Backtrace, path::PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("Datei {path} nicht gefunden in {bt}")]
    NotFound { path: PathBuf, bt: Box<Backtrace> },

    #[error("IO Fehler: {0}")]
    Io(std::io::Error),

    #[error("Serialisirungsfehler: {0}")]
    Serialize(String),

    #[error("Deserialisierungsfehler: {0}")]
    Deserialize(String),

    #[error("Konvertierungsfehler")]
    Conversion(#[from] serde_yaml::Error),

    #[error("Metadata Fehler {0}")]
    Metadata(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Property not Found {0}")]
    PropertyNotFound(String),

    #[error("Falscher Draft Status")]
    InvalidState,
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

impl StoreError {
    pub fn not_found(path: impl Into<std::path::PathBuf>) -> Self {
        StoreError::NotFound {
            path: path.into(),
            bt: Box::new(Backtrace::capture()),
        }
    }
}
