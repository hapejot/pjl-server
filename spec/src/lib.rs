//! Layer 1: Application specification types
//!
//! New types for the relationships-first architecture:
pub mod entity_spec;
pub mod relationship;

pub mod component;
pub mod data_dictionary;

pub mod package;

pub mod app;
pub mod system_user;
pub mod value_list;
pub use component::{ComponentKind, ComponentSpec, ComponentUse, TimeUnit};
pub use entity_spec::*;
pub use relationship::*;
pub use value_list::{ValueEntry, ValueList};
