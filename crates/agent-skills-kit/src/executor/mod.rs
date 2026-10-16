//! Bounded plan verification and native no-clobber copy execution.
//!
//! A successful check is an observation, never a lock or overwrite permission.

mod apply;
mod verification;

pub use apply::*;
pub use verification::*;
