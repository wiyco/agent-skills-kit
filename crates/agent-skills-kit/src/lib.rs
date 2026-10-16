//! Foundation for a local Agent Skills installer.
//!
//! Synchronous public modules separate catalog lookup, explicit-context resolution,
//! inert validation, read-only planning, approved native apply and versioned receipts.
//! CLI commands are still under development. Applications own plan review, display,
//! process exit and explicit receipt storage; the library never executes skills.
//!
//! ```
//! use agent_skills_kit::catalog::Catalog;
//! let registry = Catalog::bundled()?;
//! assert_eq!(registry.get("codex-cli").unwrap().id, "codex");
//! assert!(registry.get("unknown-product").is_none());
//! # Ok::<(), agent_skills_kit::catalog::CatalogError>(())
//! ```
//!
//! Native operations require the reviewed caller-root/private-staging trust model;
//! they do not promise arbitrary filesystem snapshots or power-loss durability.
//! Public enums are non-exhaustive; strict catalog/receipt wire formats have their
//! own explicit schema versions. Retained plans/handles are not serializable tokens.

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod filesystem;

// Shared retained-handle primitives for Windows inventory, planning and execution.
#[cfg(windows)]
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
