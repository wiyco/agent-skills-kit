//! Bounded inert metadata validation and read-only native source inventory.

mod inventory;
mod metadata;
mod yaml;

pub use inventory::*;
pub use metadata::*;
pub use yaml::{MetadataValue, MetadataValueKind, ScalarValue};
