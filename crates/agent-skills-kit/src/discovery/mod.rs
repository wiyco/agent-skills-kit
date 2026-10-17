//! Bounded observations of existing skills in explicitly selected catalog roots.
//!
//! Discovery is read-only and grants no copy, deletion or ownership authority.
//! Ancestor descriptors remain explicit rather than becoming an unbounded scan.

use std::{collections::BTreeMap, error::Error, ffi::OsString, fmt, io, path::PathBuf};

use crate::{
    catalog::{Catalog, ContextBase, DiscoveryTraversal, Scope},
    planner::{DestinationSnapshot, NamespaceError},
    resolver::{
        InstallContext, PathOrigin, ResolutionWarning, ResolveError, ResolvedDiscoveryLocation,
        ResolvedPath, resolve,
    },
    validation::{InventoryError, InventoryLimits, ValidatedSkill},
};

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
mod native;

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) fn inspect_selected(
    snapshot: &DestinationSnapshot,
    name: &std::ffi::OsStr,
    limits: InventoryLimits,
) -> Result<Box<ValidatedSkill>, DiscoveryError> {
    native::inspect_skill(snapshot, name, limits)
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) fn observe_rule(
    rule: ResolvedPath,
    context: &InstallContext,
) -> Result<DestinationSnapshot, DiscoveryError> {
    let resolved = location(rule, false, context)?;
    crate::planner::observe_destination(&resolved.anchor, &resolved.relative)
        .map_err(DiscoveryError::Namespace)
}

/// Whole-listing limits, in addition to each existing inventory budget.
#[derive(Clone, Copy, Debug)]
pub struct DiscoveryLimits {
    /// Distinct documented location labels; default 16, ceiling 64.
    pub max_locations: usize,
    /// Immediate entries across roots, including invalid entries; default 256, ceiling 4096.
    pub max_candidates: usize,
    /// Payload entries across inspections; default 16384, ceiling 65536.
    pub max_entries: usize,
    /// Regular-file inspection bytes; default 256 MiB, ceiling 1 GiB.
    pub max_read_bytes: u64,
    /// Per-candidate inspection limits, tightened to the remaining whole budget.
    pub inventory: InventoryLimits,
}
impl Default for DiscoveryLimits {
    fn default() -> Self {
        Self {
            max_locations: 16,
            max_candidates: 256,
            max_entries: 16384,
            max_read_bytes: 256 * 1024 * 1024,
            inventory: InventoryLimits::default(),
        }
    }
}
impl DiscoveryLimits {
    fn valid(self) -> bool {
        (1..=64).contains(&self.max_locations)
            && (1..=4096).contains(&self.max_candidates)
            && (1..=65536).contains(&self.max_entries)
            && (1..=1024 * 1024 * 1024).contains(&self.max_read_bytes)
            && self.inventory.validate()
    }
}

/// Why a listing or individual entry could not be completely inspected.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DiscoveryError {
    /// Unknown/unreviewed target, missing context or unsupported scope/platform.
    Resolution(ResolveError),
    /// A limit is zero or exceeds its hard ceiling.
    InvalidLimits,
    /// A finite whole-listing budget was exhausted; no successful absence is implied.
    Limit(&'static str),
    /// A directory could not be observed through reviewed native primitives.
    Namespace(NamespaceError),
    /// Unsafe, malformed, unreadable or changed skill data.
    Inventory(InventoryError),
    /// Selected root names, metadata or bindings changed during observation.
    Changed,
    /// No reviewed native listing backend exists.
    UnsupportedPlatform,
    /// A resolved path is not beneath its explicit caller context/override root.
    RootMismatch,
    /// Multiple observed names have the same conservative portable collision key.
    AmbiguousName,
    /// Native enumeration/open failure, never guessed to mean an empty directory.
    Io {
        /// Failed read operation.
        operation: &'static str,
        /// Portable I/O category.
        kind: io::ErrorKind,
        /// Native error number when available.
        raw_os_error: Option<i32>,
    },
}
impl DiscoveryError {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fn io(operation: &'static str, error: io::Error) -> Self {
        Self::Io {
            operation,
            kind: error.kind(),
            raw_os_error: error.raw_os_error(),
        }
    }
}
impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "skill discovery failed: {self:?}")
    }
}
impl Error for DiscoveryError {}

/// One observed immediate directory entry, including rejected/non-skill entries.
#[derive(Clone, Debug)]
pub struct DiscoveredSkill {
    /// Exact native basename; never silently replaced with lossy text.
    pub name: OsString,
    /// Diagnostic label under the observed container; not operation authority.
    pub path: PathBuf,
    /// Complete inert inventory, or the explicit reason it was not accepted.
    pub inspection: Result<Box<ValidatedSkill>, DiscoveryError>,
}

/// Result for one documented root; partial/errors must not appear as empty success.
#[derive(Clone, Debug)]
pub struct SkillLocation {
    /// Resolved catalog path label.
    pub path: PathBuf,
    /// Whether this is the selected tool's native write container.
    pub preferred_write: bool,
    /// Documented reader traversal roles attached to the same exact label.
    pub discovery: Vec<DiscoveryTraversal>,
    /// Missing verified root, distinct from an unreadable/unsafe root.
    pub missing: bool,
    /// Earlier root label naming the same opened directory; no second scan occurred.
    pub alias_of: Option<PathBuf>,
    /// Immediate entries in native deterministic order, including rejected entries.
    pub skills: Vec<DiscoveredSkill>,
    /// Enumeration/binding/budget failure; entries, if any, are partial observations.
    pub problem: Option<DiscoveryError>,
    snapshot: Option<DestinationSnapshot>,
    anchor: PathBuf,
    relative: PathBuf,
}
impl SkillLocation {
    /// Retained namespace preconditions, not an executable authorization token.
    pub fn snapshot(&self) -> Option<&DestinationSnapshot> {
        self.snapshot.as_ref()
    }
}

/// A selected tool/scope listing, not an authoritative view of everything a host loads.
#[derive(Clone, Debug)]
pub struct SkillListing {
    /// Canonical catalog identity, even when selected by alias.
    pub target_id: String,
    /// Explicit scope selected by the caller.
    pub scope: Scope,
    /// Native write and documented readable locations, without invented directories.
    pub locations: Vec<SkillLocation>,
    /// Host ancestor searches not expanded outside the caller-selected roots.
    pub unexpanded_ancestors: Vec<ResolvedDiscoveryLocation>,
    /// Existing source/discovery limitations and placement visibility warnings.
    pub warnings: Vec<ResolutionWarning>,
}

/// List existing skills from explicit catalog/context roots, without any writes.
///
/// Only immediate entries of the resolved roots are enumerated. Documented
/// ancestor traversal is returned separately, not silently treated as complete.
/// Missing roots are explicit empty observations; permission/unsafe/I/O failures
/// are per-location problems. All candidate names count against the budget,
/// including malformed files/directories. Failed inventory attempts consume their
/// full allocated byte/entry budget conservatively because partial read counts
/// are unavailable. Subsequent candidates remain visible as uninspected errors.
///
/// Root ancestors must remain caller-controlled under the native threat model.
/// Contents are read through retained no-follow handles. Return values are
/// observations: later copy/removal must create and revalidate its own plan.
/// No environment reads, prompts, global search, skill execution or network occur.
pub fn list_skills(
    catalog: &Catalog,
    target: &str,
    scope: Scope,
    context: &InstallContext,
    limits: DiscoveryLimits,
) -> Result<SkillListing, DiscoveryError> {
    if !limits.valid() {
        return Err(DiscoveryError::InvalidLimits);
    }
    let resolved = resolve(catalog, target, scope, context).map_err(DiscoveryError::Resolution)?;
    let mut locations = BTreeMap::new();
    locations.insert(
        resolved.preferred_write.path.clone(),
        location(resolved.preferred_write, true, context)?,
    );
    let mut unexpanded_ancestors = Vec::new();
    for reader in resolved.discovery {
        if reader.traversal != DiscoveryTraversal::Exact {
            unexpanded_ancestors.push(reader.clone());
        }
        let path = reader.location.path.clone();
        if !locations.contains_key(&path) {
            if locations.len() == limits.max_locations {
                return Err(DiscoveryError::Limit("locations"));
            }
            locations.insert(path.clone(), location(reader.location, false, context)?);
        }
        let entry = locations.get_mut(&path).expect("inserted location");
        if !entry.discovery.contains(&reader.traversal) {
            entry.discovery.push(reader.traversal);
        }
    }
    let mut listing = SkillListing {
        target_id: resolved.target_id,
        scope,
        locations: locations.into_values().collect(),
        unexpanded_ancestors,
        warnings: resolved.warnings,
    };
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    native::inspect(&mut listing, limits);
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    for location in &mut listing.locations {
        location.problem = Some(DiscoveryError::UnsupportedPlatform);
    }
    Ok(listing)
}

fn location(
    rule: ResolvedPath,
    preferred_write: bool,
    context: &InstallContext,
) -> Result<SkillLocation, DiscoveryError> {
    let root = |base| match base {
        ContextBase::Home => context.user_home.as_ref(),
        ContextBase::Project => context.project_root.as_ref(),
        ContextBase::ApplicationWorkspace => context.application_workspace.as_ref(),
    };
    let anchor = match &rule.origin {
        PathOrigin::Context(base) | PathOrigin::Fallback { base, .. } => root(*base).cloned(),
        PathOrigin::Override(variable) => context.overrides.get(variable).map(PathBuf::from),
    }
    .ok_or(DiscoveryError::RootMismatch)?;
    let relative = rule
        .path
        .strip_prefix(&anchor)
        .map_err(|_| DiscoveryError::RootMismatch)?
        .to_owned();
    Ok(SkillLocation {
        path: rule.path,
        preferred_write,
        discovery: Vec::new(),
        missing: false,
        alias_of: None,
        skills: Vec::new(),
        problem: None,
        snapshot: None,
        anchor,
        relative,
    })
}
