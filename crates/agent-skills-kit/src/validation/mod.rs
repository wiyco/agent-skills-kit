//! Bounded inert skill metadata validation, separate from filesystem inventory.

mod metadata;
mod yaml;

pub use metadata::*;
pub use yaml::{MetadataValue, MetadataValueKind, ScalarValue};
