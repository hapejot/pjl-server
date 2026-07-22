use std::fmt;

/// Error type shared by both ports - inbound adapters match on this
/// to decide the right HTTP status, outbound adapters produce it.
#[derive(Debug)]
pub enum DomainError {
    Validation(String),
    NotFound(String),
    Repository(String),
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DomainError::Validation(msg) => write!(f, "Validation error: {msg}"),
            DomainError::NotFound(msg) => write!(f, "Not found: {msg}"),
            DomainError::Repository(msg) => write!(f, "Repository error: {msg}"),
        }
    }
}

impl std::error::Error for DomainError {}
