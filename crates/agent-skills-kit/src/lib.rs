//! Foundation for a local Agent Skills installer.
//!
//! Installation APIs are not implemented yet. The API remains provisional
//! pending its implementation and review.

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod filesystem;

pub mod catalog;
pub mod planner;
pub mod resolver;
pub mod validation;

/// Describes the current bootstrap capability without claiming install support.
pub const STATUS: &str = "Agent Skills Kit bootstrap: installation is not implemented yet.";
