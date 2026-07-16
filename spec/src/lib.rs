//! Layer 1: Application specification types
//!
//! New types for the relationships-first architecture:
pub mod entity_spec;
pub mod relationship;

pub mod data_dictionary;

pub mod package;

pub mod app;
pub use entity_spec::*;
pub use relationship::*;
