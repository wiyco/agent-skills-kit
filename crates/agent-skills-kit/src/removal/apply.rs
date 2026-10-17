use super::*;
use std::io;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "unix.rs"]
mod unix;
#[cfg(windows)]
#[path = "windows.rs"]
mod windows;

/// Structured native removal failure; partial outcomes are retained separately.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemovalFailure {
    /// Relevant exact native label; may be a staging diagnostic after detachment.
    pub path: PathBuf,
    /// Native operation or precondition that failed.
    pub operation: &'static str,
    /// Typed failure class.
    pub kind: RemovalFailureKind,
}
/// Failure category for an approved permanent-removal operation.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RemovalFailureKind {
    /// Plan/contents/reader evidence no longer satisfies the approved preview.
    Preflight(RemovalError),
    /// Object identity, content, mode or namespace binding changed.
    Changed,
    /// A required reviewed native capability or safe permission policy is absent.
    Unsupported(&'static str),
    /// Native I/O refusal; no force/permission repair is implied.
    Io {
        /// Portable I/O category.
        kind: io::ErrorKind,
        /// Native error number, if supplied.
        raw_os_error: Option<i32>,
    },
}
impl fmt::Display for RemovalFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "permanent removal failed during {}: {:?}",
            self.operation, self.kind
        )
    }
}
impl Error for RemovalFailure {}
/// What the operation actually established about one intended removal.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RemovalEntryState {
    /// Native namespace removal completed for the approved object.
    Removed,
    /// No unlink was attempted; presence/contents are not asserted current.
    NotAttempted,
    /// Removal failed after establishing the approved entry is still present.
    Remaining,
    /// Current presence/identity could not be safely established.
    Unknown,
}
/// Per-entry permanent-removal outcome, including the selected root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemovalEntryOutcome {
    /// Original exact native label of this intended entry.
    pub path: PathBuf,
    /// Evidence-based result; never a rollback promise.
    pub state: RemovalEntryState,
    /// Native/validation failure for this entry, if attempted.
    pub failure: Option<RemovalFailure>,
}
/// Accurate progress even when an approved operation fails or is partially applied.
#[derive(Clone, Debug)]
pub struct RemovalReport {
    /// Selected original payload label, not a statement that it is now absent.
    pub original_path: PathBuf,
    /// Every approved descendant plus root, including not-attempted entries.
    pub outcomes: Vec<RemovalEntryOutcome>,
    /// First operation failure, without losing earlier removals.
    pub failure: Option<RemovalFailure>,
    /// At least one native permanent-removal call was attempted, even if it failed.
    pub permanent_removal_attempted: bool,
    /// Bounded observed unapproved names/objects in the private enclosure.
    /// Diagnostic labels are not authority or a claim that they remain current.
    pub unrecognized_entries: Vec<PathBuf>,
    /// A diagnostic scan encountered lost binding, I/O failure or its finite limit.
    pub unrecognized_scan_incomplete: bool,
    /// Private enclosure retained for manual inspection, when its label is known.
    pub retained_stage: Option<PathBuf>,
    /// The recorded stage label could not be confirmed and may name unrelated data.
    /// Never use it as automatic cleanup authority.
    pub stage_location_uncertain: bool,
    /// The detached object was restored to the original name before any unlink.
    /// It may be a refused replacement, not the originally approved object.
    pub restored_before_removal: bool,
    /// Additional restoration/cleanup failures or lost-binding diagnostics.
    pub cleanup_failures: Vec<RemovalFailure>,
}
impl RemovalReport {
    /// True only for complete confirmed payload removal and successful stage cleanup.
    pub fn is_successful(&self) -> bool {
        !self.stage_location_uncertain
            && !self.unrecognized_scan_incomplete
            && self.unrecognized_entries.is_empty()
            && self.failure.is_none()
            && self.retained_stage.is_none()
            && self.cleanup_failures.is_empty()
            && self
                .outcomes
                .iter()
                .all(|entry| entry.state == RemovalEntryState::Removed)
    }
}
fn initial(plan: &RemovalPlan) -> RemovalReport {
    let mut outcomes = plan
        .entries
        .iter()
        .map(|entry| RemovalEntryOutcome {
            path: plan.path.join(&entry.relative_path),
            state: RemovalEntryState::NotAttempted,
            failure: None,
        })
        .collect::<Vec<_>>();
    outcomes.push(RemovalEntryOutcome {
        path: plan.path.clone(),
        state: RemovalEntryState::NotAttempted,
        failure: None,
    });
    RemovalReport {
        original_path: plan.path.clone(),
        outcomes,
        failure: None,
        permanent_removal_attempted: false,
        unrecognized_entries: vec![],
        unrecognized_scan_incomplete: false,
        retained_stage: None,
        stage_location_uncertain: false,
        restored_before_removal: false,
        cleanup_failures: vec![],
    }
}
/// Permanently remove only an explicitly approved, current payload.
///
/// The caller must present the complete preview, control context/container/ancestor
/// namespaces and stop skill writers. No undo, arbitrary mutable-tree snapshot,
/// secure erasure or automatic recovery is promised. Every native backend must
/// pass ADR 0009's evidence gates. Eligible Windows NTFS application uses held
/// DELETE-capable objects and confirms namespace removal after closing handles.
/// Never infer ownership from receipts/prefixes or use copy approval for removal.
pub fn apply_removal(approved: ApprovedRemoval) -> RemovalReport {
    let plan = approved.plan;
    let mut report = initial(&plan);
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    if let Err(error) = super::plan::verify(&plan) {
        report.failure = Some(RemovalFailure {
            path: plan.path.clone(),
            operation: "read-only removal preflight",
            kind: RemovalFailureKind::Preflight(error),
        });
        return report;
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    unix::apply(&plan, &mut report);
    #[cfg(windows)]
    windows::apply(&plan, &mut report);
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        report.failure = Some(RemovalFailure {
            path: plan.path.clone(),
            operation: "native permanent removal",
            kind: RemovalFailureKind::Unsupported(
                "native removal backend not yet enabled for this platform",
            ),
        });
    }
    report
}
