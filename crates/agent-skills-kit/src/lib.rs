//! Foundation for a local Agent Skills installer.
//!
//! Library APIs remain provisional pending the complete native safety matrix
//! and external-consumer review. CLI commands are still under development.

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod filesystem;

// Native primitives are tested before the public Windows backend is enabled.
#[cfg(all(windows, test))]
mod filesystem_windows;

pub mod catalog;
pub mod executor;
pub mod planner;
pub mod receipt;
pub mod resolver;
pub mod validation;

/// Describes the development CLI capability without claiming host discovery.
pub const STATUS: &str =
    "Agent Skills Kit development build: CLI commands are not implemented yet.";
