//! Read-only destination observations for subsequent install-plan construction.
//!
//! Observations are preconditions, not write authorization or atomic snapshots.

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod names;
mod namespace;
mod plan;

pub use namespace::*;
pub use plan::*;
