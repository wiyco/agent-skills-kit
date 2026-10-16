use std::{fmt, io, path::PathBuf};

use super::{VerificationError, VerificationLimits, verify_plan};
use crate::planner::InstallPlan;

/// Explicit approval of a plan through calling [`apply_plan`]; never force permission.
#[derive(Clone, Copy, Debug)]
pub struct ApplyOptions {
    /// Finite preflight/reader bounds, including large custom payloads.
    pub verification: VerificationLimits,
    /// Stop after the first failed destination; default true. Earlier successes remain.
    pub stop_on_failure: bool,
    /// Total approved file bytes copied across destinations; default 256 MiB,
    /// hard ceiling 1 GiB. Revalidation reads are separately bounded.
    pub max_copy_bytes: u64,
}
impl Default for ApplyOptions {
    fn default() -> Self {
        Self {
            verification: VerificationLimits::default(),
            stop_on_failure: true,
            max_copy_bytes: 256 * 1024 * 1024,
        }
    }
}

/// Attribution copied into a report without requiring access to the original plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutcomeAttribution {
    /// Approved source path label.
    pub source: PathBuf,
    /// Canonical selected target ID.
    pub target_id: String,
}

/// One physical destination's outcome, retaining all requested attribution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DestinationOutcome {
    /// Planned native destination label, absent for unresolved requests.
    pub path: Option<PathBuf>,
    /// Every source/target represented by this operation.
    pub attributions: Vec<OutcomeAttribution>,
    /// Explicit result; success is local to this destination.
    pub status: OutcomeStatus,
}

/// Result of an individual operation; no cross-target transaction is promised.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OutcomeStatus {
    /// Exact staged payload and required destination permissions were completed.
    Created,
    /// Approved existing payload was revalidated and left untouched.
    Unchanged,
    /// Operation failed before payload commit, with existing content preserved.
    Failed(ApplyError),
    /// Complete payload was committed without replacement, but destination
    /// permission finalization failed or may be partial. It remains installed;
    /// this error never authorizes deleting it or rolling back earlier targets.
    CommittedWithPermissionFailure(ApplyError),
    /// Preflight failed or stop-on-failure prevented this operation.
    NotAttempted,
}

impl OutcomeStatus {
    /// Whether this operation observed a payload commit, even if later permission
    /// finalization failed. This is historical evidence, never cleanup authority.
    pub fn payload_was_committed(&self) -> bool {
        matches!(
            self,
            Self::Created | Self::CommittedWithPermissionFailure(_)
        )
    }
}

/// Full batch result, including cleanup diagnostics independently of commit status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallReport {
    /// Whole-plan failure before any destination write.
    pub preflight_error: Option<VerificationError>,
    /// Every planned physical operation, in deterministic plan order.
    pub outcomes: Vec<DestinationOutcome>,
    /// Owned paths left behind, or failed identity checks; never hidden by success.
    pub cleanup_errors: Vec<ApplyError>,
}
impl InstallReport {
    /// All operations succeeded or were unchanged, with no cleanup uncertainty.
    pub fn is_successful(&self) -> bool {
        self.preflight_error.is_none()
            && self.cleanup_errors.is_empty()
            && self.outcomes.iter().all(|outcome| {
                matches!(
                    outcome.status,
                    OutcomeStatus::Created | OutcomeStatus::Unchanged
                )
            })
    }
}

/// Native copy/commit/cleanup failure with an operation and native code when available.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplyError {
    /// Relevant native path label.
    pub path: PathBuf,
    /// Typed reason.
    pub kind: ApplyErrorKind,
}
/// Why one approved operation could not finish safely.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ApplyErrorKind {
    /// Approved source bytes, entry identity or modes changed.
    SourceChanged,
    /// A source reread failed, retaining its typed native/validation cause.
    SourceInspection(crate::validation::InventoryError),
    /// Approved namespace/absence changed or ownership is uncertain.
    DestinationChanged,
    /// A namespace recheck failed, retaining its native/validation cause.
    Namespace(crate::planner::NamespaceError),
    /// Existing payload reread failed, retaining its native/validation cause.
    DestinationInspection(crate::validation::InventoryError),
    /// Stage identity/content no longer matches operation ownership.
    StageChanged,
    /// A newly created destination parent remains for explicit recovery;
    /// automatic deletion outside private staging is deliberately avoided.
    CreatedParentRetained,
    /// Unsupported permission/ACL/filesystem behavior, never an ordinary fallback.
    Unsupported(&'static str),
    /// Native I/O or capability error.
    Io {
        /// Exact failed operation.
        operation: &'static str,
        /// Portable native I/O classification.
        kind: io::ErrorKind,
        /// Original errno, when supplied by the OS.
        raw_os_error: Option<i32>,
    },
}
impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "native installation failed: {:?}", self.kind)
    }
}
impl std::error::Error for ApplyError {}

/// Apply an explicitly approved immutable plan using bounded native copy semantics.
///
/// The caller must control approved roots/ancestors and operation-private staging
/// under the reviewed threat model. No existing content is overwritten; native
/// exclusive rename is mandatory. The call never executes payloads or persists
/// receipts. Earlier successful destinations survive a later failure. A process
/// interruption can leave private staging; no crash-durability guarantee is made.
/// Unknown ownership prevents cleanup rather than authorizing deletion by prefix.
/// Windows fails preflight before writes; no path-based move fallback exists.
pub fn apply_plan(plan: &InstallPlan, options: ApplyOptions) -> InstallReport {
    let mut report = initial_report(plan);
    // Read-only verification is a separate capability from native copy execution.
    if !cfg!(any(target_os = "linux", target_os = "macos")) {
        report.preflight_error = Some(VerificationError {
            path: None,
            kind: super::VerificationErrorKind::UnsupportedPlatform,
        });
        return report;
    }
    if let Err(error) = verify_plan(plan, options.verification) {
        report.preflight_error = Some(error);
        return report;
    }
    let bytes = plan
        .intended_writes()
        .iter()
        .try_fold(0u64, |total, write| total.checked_add(write.bytes));
    if !(1..=1024 * 1024 * 1024).contains(&options.max_copy_bytes)
        || bytes.is_none_or(|bytes| bytes > options.max_copy_bytes)
    {
        report.preflight_error = Some(VerificationError {
            path: None,
            kind: super::VerificationErrorKind::Limit("copy bytes"),
        });
        return report;
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    native::apply(plan, options, &mut report);
    report
}

fn initial_report(plan: &InstallPlan) -> InstallReport {
    InstallReport {
        preflight_error: None,
        outcomes: plan
            .destinations()
            .iter()
            .map(|destination| DestinationOutcome {
                path: destination.path().map(ToOwned::to_owned),
                attributions: destination
                    .attributions()
                    .iter()
                    .map(|attribution| OutcomeAttribution {
                        source: plan.sources()[attribution.source_index]
                            .inventory()
                            .requested_root()
                            .to_owned(),
                        target_id: plan.targets()[attribution.target_index].id().to_owned(),
                    })
                    .collect(),
                status: OutcomeStatus::NotAttempted,
            })
            .collect(),
        cleanup_errors: Vec::new(),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "apply_unix.rs"]
mod native;
