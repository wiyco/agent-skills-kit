//! Bounded execution preflight. Copy/commit behavior is not implemented yet.
//!
//! A successful check is an observation, never a lock or overwrite permission.

mod verification;

pub use verification::*;
