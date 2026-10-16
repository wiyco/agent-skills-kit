//! Versioned target metadata and offline validation, without path resolution.

mod bundled;
mod model;
mod names;
mod validation;

pub use model::*;
pub use names::ReservedSkillName;
pub use validation::{Catalog, CatalogError};

/// The only catalog document version currently understood.
pub const SCHEMA_VERSION: u32 = 1;
