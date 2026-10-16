//! Read-only destination observations for subsequent install-plan construction.
//!
//! Observations are preconditions, not write authorization or atomic snapshots.

mod namespace;

pub use namespace::*;
