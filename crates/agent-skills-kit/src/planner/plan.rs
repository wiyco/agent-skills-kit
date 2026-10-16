use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

use super::{DestinationSnapshot, NamespaceError};
use crate::{
    catalog::{Catalog, RuleEvidence, Scope},
    resolver::{InstallContext, ResolutionWarning, ResolveError, ResolvedDestination},
    validation::{InventoryError, InventoryLimits, TargetConstraintError, ValidatedSkill},
};

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
#[path = "build.rs"]
mod build;

/// Explicit inputs to a read-only preview. No targets/scope/context are inferred.
pub struct InstallRequest<'a> {
    /// Previously validated sources; their complete snapshots are revalidated.
    pub skills: &'a [ValidatedSkill],
    /// Target IDs/aliases; repeated canonical targets are selected once.
    pub target_ids: &'a [&'a str],
    /// Explicit common installation scope.
    pub scope: Scope,
    /// Caller-provided roots and overrides, never process-global discovery.
    pub context: &'a InstallContext,
    /// Additional finite limits for the whole preview.
    pub limits: PlanLimits,
}

/// Whole-preview limits in addition to each bounded inventory inspection.
#[derive(Clone, Copy, Debug)]
pub struct PlanLimits {
    /// Raw source selection count; default 32, ceiling 128.
    pub max_skills: usize,
    /// Raw target selection count; default 64, ceiling 256.
    pub max_targets: usize,
    /// Source/target combinations; default 128, ceiling 1024.
    pub max_destinations: usize,
    /// Total files/directories inspected; default 16384, ceiling 65536.
    pub max_inspected_entries: usize,
    /// Total regular-file bytes read again; default 256 MiB, ceiling 1 GiB.
    pub max_read_bytes: u64,
    /// Aggregate recorded path bytes; default 16 MiB, ceiling 64 MiB.
    pub max_path_bytes: usize,
    /// Logical final entry/parent writes; default 16384, ceiling 65536.
    pub max_intended_entries: usize,
    /// Per-inspection limits; source validation uses the tighter remaining budget.
    pub inventory: InventoryLimits,
}
impl Default for PlanLimits {
    fn default() -> Self {
        Self {
            max_skills: 32,
            max_targets: 64,
            max_destinations: 128,
            max_inspected_entries: 16384,
            max_read_bytes: 256 * 1024 * 1024,
            max_path_bytes: 16 * 1024 * 1024,
            max_intended_entries: 16384,
            inventory: InventoryLimits::default(),
        }
    }
}
impl PlanLimits {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fn valid(self) -> bool {
        (1..=128).contains(&self.max_skills)
            && (1..=256).contains(&self.max_targets)
            && (1..=1024).contains(&self.max_destinations)
            && (1..=65536).contains(&self.max_inspected_entries)
            && (1..=1024 * 1024 * 1024).contains(&self.max_read_bytes)
            && (1..=64 * 1024 * 1024).contains(&self.max_path_bytes)
            && (1..=65536).contains(&self.max_intended_entries)
            && self.inventory.validate()
    }
}

/// One canonical requested target, retaining evidence even when resolution fails.
#[derive(Clone, Debug)]
pub struct PlannedTarget {
    pub(super) id: String,
    pub(super) resolution: Result<ResolvedDestination, ResolveError>,
    pub(super) evidence: Vec<RuleEvidence>,
    pub(super) warnings: Vec<ResolutionWarning>,
}
impl PlannedTarget {
    /// Canonical ID, or unchanged unknown ID if the catalog has no match.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Pure library resolution or its typed unsupported/context failure.
    pub fn resolution(&self) -> Result<&ResolvedDestination, &ResolveError> {
        self.resolution.as_ref()
    }
    /// Reviewed catalog rule evidence, never a host-discovery success claim.
    pub fn evidence(&self) -> &[RuleEvidence] {
        &self.evidence
    }
    /// Shared-reader and source/version/configuration limitations.
    pub fn warnings(&self) -> &[ResolutionWarning] {
        &self.warnings
    }
}

/// A complete source/target attribution referencing this plan's sorted tables.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct TargetAttribution {
    /// Index in [`InstallPlan::sources`].
    pub source_index: usize,
    /// Index in [`InstallPlan::targets`].
    pub target_index: usize,
}

/// Conservative per-destination preview classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PlannedAction {
    /// Destination absent; this records intent, not proven write permission.
    Create,
    /// Complete existing payload bytes, entry set and ordinary modes match.
    Unchanged,
    /// Existing or competing content is not approved for replacement.
    Conflict,
    /// Target/backend/host constraint is unsupported.
    Unsupported,
}

/// Reason an operation cannot be approved as an ordinary create.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DestinationProblem {
    /// Target resolution failed; catalog evidence remains in the target table.
    Target(ResolveError),
    /// General-valid metadata violates a separate host-specific constraint.
    Host(TargetConstraintError),
    /// Namespace could not be safely inspected; no absence is guessed.
    Namespace(NamespaceError),
    /// Existing entry set, bytes or modes differ from the intended payload.
    ExistingContentDiffers,
    /// Existing payload could not be completely and safely inspected.
    ExistingInspection(InventoryError),
    /// Native lookup used an alias of the requested skill directory spelling.
    ExistingNameMismatch,
    /// Several selected sources request different payloads at one physical path.
    CompetingSources,
    /// An existing name has a different spelling under the portable collision key.
    PortableNameCollision {
        /// First conflicting native path spelling.
        first: PathBuf,
        /// Second conflicting native path spelling.
        second: PathBuf,
    },
}

/// Logical intended write kind; private staging is part of copy execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum IntendedWriteKind {
    /// Missing destination ancestor, with owner-only initial permission intent.
    CreateParent,
    /// Final skill root directory; execution stages the payload before commit.
    CreateSkillRoot,
    /// Supporting directory, including an empty directory.
    CreateDirectory,
    /// Exact source bytes copied as data, not regenerated metadata.
    CopyFile,
}

/// Every logical payload/parent write of an applicable create preview.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntendedWrite {
    /// Absolute final path. Transient owned staging is not a persistent payload.
    pub path: PathBuf,
    /// Logical operation.
    pub kind: IntendedWriteKind,
    /// Non-broadening ordinary output mode, or 0700 for new parents.
    pub mode: u32,
    /// File byte length; zero for directories.
    pub bytes: u64,
    /// BLAKE3-256 file integrity digest; absent for directories.
    pub digest: Option<[u8; 32]>,
}

/// One physical destination or one unresolved source/target request.
#[derive(Clone, Debug)]
pub struct PlannedDestination {
    pub(super) path: Option<PathBuf>,
    pub(super) action: PlannedAction,
    pub(super) problems: Vec<DestinationProblem>,
    pub(super) attributions: Vec<TargetAttribution>,
    pub(super) source_index: usize,
    pub(super) namespace: Option<DestinationSnapshot>,
    pub(super) writes: Vec<IntendedWrite>,
    pub(super) existing: Option<ValidatedSkill>,
}
impl PlannedDestination {
    /// Canonical observed candidate, or requested spelling when observation failed.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
    /// Conservative classification; conflict/unsupported never authorizes overwrite.
    pub fn action(&self) -> PlannedAction {
        self.action
    }
    /// Typed diagnostic for non-create destinations.
    pub fn problem(&self) -> Option<&DestinationProblem> {
        self.problems.first()
    }
    /// All known reasons at this physical destination, without dropping merged findings.
    pub fn problems(&self) -> &[DestinationProblem] {
        &self.problems
    }
    /// All selected canonical targets and sources, preserved after physical deduplication.
    pub fn attributions(&self) -> &[TargetAttribution] {
        &self.attributions
    }
    /// Representative source for equivalent payloads at this destination.
    pub fn source_index(&self) -> usize {
        self.source_index
    }
    /// Retained selected namespace preconditions, when safely observed.
    pub fn namespace(&self) -> Option<&DestinationSnapshot> {
        self.namespace.as_ref()
    }
    /// Logical final payload and missing-parent writes; no writes have occurred.
    pub fn intended_writes(&self) -> &[IntendedWrite] {
        &self.writes
    }
    /// Complete retained existing-payload preconditions for an unchanged result.
    ///
    /// Execution must revalidate them; these are observations, not a content lock.
    pub fn existing_payload(&self) -> Option<&ValidatedSkill> {
        self.existing.as_ref()
    }
}

/// Immutable, non-fabricable read-only preview with owned revalidated sources.
#[derive(Debug)]
pub struct InstallPlan {
    pub(super) scope: Scope,
    pub(super) sources: Vec<ValidatedSkill>,
    pub(super) targets: Vec<PlannedTarget>,
    pub(super) destinations: Vec<PlannedDestination>,
    pub(super) inspected_bytes: u64,
    pub(super) writes: Vec<IntendedWrite>,
}
impl InstallPlan {
    /// Explicit scope shared by the selection.
    pub fn scope(&self) -> Scope {
        self.scope
    }
    /// Revalidated sources, sorted by canonical path and deduplicated by identity.
    pub fn sources(&self) -> &[ValidatedSkill] {
        &self.sources
    }
    /// Canonical targets, including unsupported/unknown selections, sorted by ID.
    pub fn targets(&self) -> &[PlannedTarget] {
        &self.targets
    }
    /// Deterministically ordered physical destinations and unresolved requests.
    pub fn destinations(&self) -> &[PlannedDestination] {
        &self.destinations
    }
    /// Accounted reread bytes, excluding prior caller validation.
    ///
    /// Successful inspections charge actual bytes. Failed existing inspections
    /// reserve their full allowance, so this can be a conservative upper bound.
    pub fn inspected_bytes(&self) -> u64 {
        self.inspected_bytes
    }
    /// Deduplicated logical writes, including shared missing-parent requirements.
    pub fn intended_writes(&self) -> &[IntendedWrite] {
        &self.writes
    }
    /// Whether every requested operation is an eligible create or unchanged payload.
    ///
    /// This never authorizes applying a stale plan or replacing existing content.
    pub fn is_applicable(&self) -> bool {
        !self.destinations.is_empty()
            && self.destinations.iter().all(|destination| {
                matches!(
                    destination.action,
                    PlannedAction::Create | PlannedAction::Unchanged
                ) && destination.problems.is_empty()
            })
    }
}

/// Fatal structural/budget/source failure; no partial plan or writes are returned.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PlanError {
    /// No reviewed native planner backend is available.
    UnsupportedPlatform,
    /// Plan limits are zero or exceed documented ceilings.
    InvalidLimits,
    /// Explicit source/target selection is required.
    EmptySelection(&'static str),
    /// A whole-preview resource budget was exceeded.
    Limit(&'static str),
    /// A previously validated source is no longer the approved snapshot.
    StaleSource(PathBuf),
    /// Source could not be safely reread; no old snapshot is silently reused.
    SourceInspection(InventoryError),
    /// Fixed native ancestry/namespace inspection could not establish safety.
    Namespace(NamespaceError),
    /// A portable case/normalization collision makes payload placement ambiguous.
    PortableNameCollision {
        /// First conflicting native path spelling.
        first: PathBuf,
        /// Second conflicting native path spelling.
        second: PathBuf,
    },
    /// Source/source, source/destination or destination/destination roots overlap.
    Overlap {
        /// First selected root involved in containment.
        first: PathBuf,
        /// Second selected root involved in containment.
        second: PathBuf,
    },
}
impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "install planning failed: {self:?}")
    }
}
impl Error for PlanError {}

/// Build a bounded immutable preview without filesystem writes or payload execution.
///
/// Canonical targets and physical source identities are deduplicated; distinct
/// native host placements stay distinct. Full source snapshots are revalidated.
/// Root overlaps are rejected using native ancestry and selected component keys.
/// Existing directories are unchanged only after complete retained-handle
/// comparison of bytes, entry sets and intended modes.
/// Eligible Windows NTFS observations use the same planning policy. Execution
/// remains a separate platform capability gate. An applicable preview is intent,
/// not permission to bypass immediate native apply checks or force an overwrite.
pub fn plan_install(
    catalog: &Catalog,
    request: InstallRequest<'_>,
) -> Result<InstallPlan, PlanError> {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    {
        build::plan(catalog, request)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (catalog, request);
        Err(PlanError::UnsupportedPlatform)
    }
}
