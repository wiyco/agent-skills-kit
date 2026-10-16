use std::{error::Error, fmt, path::PathBuf};

use crate::{
    planner::{InstallPlan, NamespaceError},
    validation::{InventoryError, InventoryLimits},
};

/// Explicit finite budgets for one complete execution preflight.
#[derive(Clone, Copy, Debug)]
pub struct VerificationLimits {
    /// Per-source/existing-payload reader limits; callers can tighten or raise
    /// these within the reader's hard ceilings. Defaults to ordinary inventory.
    pub inventory: InventoryLimits,
    /// All reread bytes; default 256 MiB, hard ceiling 1 GiB.
    pub max_read_bytes: u64,
    /// All payload entries; default 16384, hard ceiling 65536.
    pub max_entries: usize,
}
impl Default for VerificationLimits {
    fn default() -> Self {
        Self {
            inventory: InventoryLimits::default(),
            max_read_bytes: 256 * 1024 * 1024,
            max_entries: 16384,
        }
    }
}

/// Successful bounded observation counts, without write authorization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerificationReport {
    /// Complete approved sources inspected again.
    pub sources_checked: usize,
    /// Destination namespace/content preconditions inspected again.
    pub destinations_checked: usize,
    /// Actual regular-file bytes reread in successful inspections.
    pub read_bytes: u64,
    /// Actual payload entries inspected below roots.
    pub entries: usize,
}

/// Typed preflight failure; a failure never authorizes replacement or replanning.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationError {
    /// Relevant source/destination label, when the failure concerns one path.
    pub path: Option<PathBuf>,
    /// Structured failure classification.
    pub kind: VerificationErrorKind,
}

/// Why an immutable approved preview cannot currently proceed.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum VerificationErrorKind {
    /// No reviewed native backend on this platform; no filesystem inspection.
    UnsupportedPlatform,
    /// At least one planned request already conflicts or is unsupported.
    InapplicablePlan,
    /// Zero/excessive inventory or whole-check budgets.
    InvalidLimits,
    /// Whole-check budget exhausted; no partial success report.
    Limit(&'static str),
    /// A previously approved source no longer has the same snapshot.
    SourceChanged,
    /// Source could not be safely and completely inspected.
    SourceInspection(InventoryError),
    /// A selected namespace binding or absence changed.
    Namespace(NamespaceError),
    /// An unchanged destination no longer has the approved content/identity/modes.
    DestinationChanged,
    /// Existing payload could not be safely and completely inspected.
    DestinationInspection(InventoryError),
}
impl fmt::Display for VerificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "plan verification failed: {:?}", self.kind)
    }
}
impl Error for VerificationError {}

/// Recheck every approved source and selected destination without writes.
///
/// The caller must still control approved roots and ancestors under the threat
/// model. Source roots retain their original pathname binding. Existing payloads
/// are reread through retained handles and namespace bindings checked again.
/// Complete entry identities, types, modes, lengths and digests must match; a
/// newly identical replacement is still stale. Custom large plans may require
/// explicit larger reader limits. No process environment or payload is executed.
///
/// This neither freezes content nor creates a reusable authorization token.
/// Execution must check relevant preconditions immediately around its own native
/// operations. This API itself never stages, commits, cleans up or overwrites.
/// Eligible Windows NTFS plans use the same bounded verification policy.
pub fn verify_plan(
    plan: &InstallPlan,
    limits: VerificationLimits,
) -> Result<VerificationReport, VerificationError> {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    {
        native::verify(plan, limits)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (plan, limits);
        Err(VerificationError {
            path: None,
            kind: VerificationErrorKind::UnsupportedPlatform,
        })
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
#[path = "verification_native.rs"]
mod native;
