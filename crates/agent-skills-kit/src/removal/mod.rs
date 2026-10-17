//! Permanent-removal previews, operation-specific approval and native application.
//!
//! Unix and Windows application remain separately gated by native evidence.
//! Read-only preview/approval never substitutes for apply-time revalidation.
//! Receipt data is never authority.
//! See ADR 0009 for the caller-controlled namespace and stopped-writer contract.

use crate::{
    catalog::{Catalog, Scope},
    discovery::{DiscoveryError, SkillListing},
    planner::{DestinationPresence, DestinationSnapshot},
    resolver::InstallContext,
    validation::{EntryStamp, InventoryLimits, SourceEntry},
};
use std::{
    error::Error,
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
};

mod apply;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
mod plan;
pub use apply::*;

/// Bounds for the complete removal preview, not permission to remove extra data.
#[derive(Clone, Copy, Debug)]
pub struct RemovalLimits {
    /// Complete selected payload inspection; existing inventory ceilings apply.
    pub inventory: InventoryLimits,
    /// Catalog targets considered for known reader impact; default/ceiling 256/1024.
    pub max_targets: usize,
    /// Resolved location observations across scopes; default/ceiling 1024/4096.
    pub max_locations: usize,
}
impl Default for RemovalLimits {
    fn default() -> Self {
        Self {
            inventory: InventoryLimits::default(),
            max_targets: 256,
            max_locations: 1024,
        }
    }
}
/// Explicit current selection consumed by read-only removal planning.
#[derive(Debug)]
pub struct RemovalRequest {
    /// Successful bounded listing, not a serialized receipt or path token.
    pub listing: SkillListing,
    /// Zero-based location index into that exact listing.
    pub location_index: usize,
    /// Exact selected immediate basename.
    pub skill_name: OsString,
    /// Explicit context for source verification and catalog-known reader impact.
    pub context: InstallContext,
    /// Finite inspection/impact budgets.
    pub limits: RemovalLimits,
}
/// Why a complete removal preview or its explicit approval cannot be obtained.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RemovalError {
    /// No reviewed native planning backend exists.
    UnsupportedPlatform,
    /// Zero or excessive limits.
    InvalidLimits,
    /// A finite catalog/location budget was exceeded.
    Limit(&'static str),
    /// The selected index/name/root was missing, partial or an alias.
    InvalidSelection,
    /// Source location does not belong to the selected catalog target/context.
    ContextMismatch,
    /// The payload equals/contains an explicit context or resolved host container.
    ProtectedRoot(PathBuf),
    /// Selection/root/contents changed after discovery.
    Changed,
    /// Native namespace or complete inventory inspection failed.
    Discovery(DiscoveryError),
    /// The operation-specific fingerprint does not match this complete plan.
    ConfirmationMismatch,
    /// Lossless native preview encoding failed.
    Encoding(String),
}
impl fmt::Display for RemovalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "permanent removal planning failed: {self:?}")
    }
}
impl Error for RemovalError {}

/// Catalog-known reader of the selected physical skills container.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemovalReader {
    /// Canonical catalog target identity.
    pub target_id: String,
    /// Explicitly resolved scope.
    pub scope: Scope,
    /// Observed container label, not exclusive host visibility.
    pub path: PathBuf,
    /// Whether this role is the target's native write location.
    pub preferred_write: bool,
    /// Ancestor traversal was documented but not expanded.
    pub ancestors_unexpanded: bool,
}
/// Reader evidence that could not be fully resolved/observed in explicit context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnresolvedRemovalReader {
    /// Canonical target identity.
    pub target_id: String,
    /// Requested scope.
    pub scope: Scope,
    /// Known label when resolution succeeded.
    pub path: Option<PathBuf>,
    /// Explicit diagnostic; prose is not a stable machine error code.
    pub reason: String,
}
/// Immutable, complete preview; no write/ownership authority until explicit approval.
#[derive(Debug)]
pub struct RemovalPlan {
    target_id: String,
    scope: Scope,
    path: PathBuf,
    name: OsString,
    root_stamp: EntryStamp,
    entries: Vec<SourceEntry>,
    metadata: Vec<u8>,
    bytes: u64,
    namespace: DestinationSnapshot,
    readers: Vec<RemovalReader>,
    unresolved: Vec<UnresolvedRemovalReader>,
    token: String,
    limits: RemovalLimits,
    catalog: Catalog,
    context: InstallContext,
}
impl RemovalPlan {
    /// Canonical selected source target.
    pub fn target_id(&self) -> &str {
        &self.target_id
    }
    /// Explicit selected source scope.
    pub fn scope(&self) -> Scope {
        self.scope
    }
    /// Exact current payload label, not a persistent ownership token.
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Exact immediate basename to be removed.
    pub fn name(&self) -> &std::ffi::OsStr {
        &self.name
    }
    /// Selected root preconditions; the root itself is also an intended removal.
    pub fn root_stamp(&self) -> &EntryStamp {
        &self.root_stamp
    }
    /// Every intended descendant removal; root is separately identified by path().
    pub fn entries(&self) -> &[SourceEntry] {
        &self.entries
    }
    /// Original inert SKILL.md bytes included in the approved fingerprint.
    pub fn metadata_bytes(&self) -> &[u8] {
        &self.metadata
    }
    /// Complete inspected regular-file payload size.
    pub fn total_bytes(&self) -> u64 {
        self.bytes
    }
    /// Catalog-known physical-container readers, not proof of actual host loading.
    pub fn readers(&self) -> &[RemovalReader] {
        &self.readers
    }
    /// Explicit unresolved reader/context evidence; never silently omitted.
    pub fn unresolved_readers(&self) -> &[UnresolvedRemovalReader] {
        &self.unresolved
    }
    /// Operation/version-specific fingerprint; not a secret or ownership credential.
    pub fn confirmation_token(&self) -> &str {
        &self.token
    }
    /// Finite budgets used to derive the plan; future apply must respect them.
    pub fn limits(&self) -> RemovalLimits {
        self.limits
    }
    /// Recheck retained container bindings only; this does not freeze payload bytes.
    pub fn namespace_is_current(&self) -> Result<bool, RemovalError> {
        self.namespace
            .is_current()
            .map_err(|error| RemovalError::Discovery(DiscoveryError::Namespace(error)))
    }
    /// Explicitly approve permanent removal of this exact preview.
    ///
    /// Call only after presenting all contents/readers, permanent/no-undo warning
    /// and the obligation to stop writers. Generic copy approval is insufficient.
    /// No filesystem writes occur here. Native apply must revalidate independently.
    pub fn approve_permanent_removal(
        self,
        confirmation: &str,
    ) -> Result<ApprovedRemoval, RemovalError> {
        if confirmation != self.token {
            return Err(RemovalError::ConfirmationMismatch);
        }
        Ok(ApprovedRemoval { plan: self })
    }
}
/// Consumed, operation-specific approval; cannot be built from a receipt or JSON.
#[derive(Debug)]
pub struct ApprovedRemoval {
    plan: RemovalPlan,
}
impl ApprovedRemoval {
    /// Exact immutable approved preview; not a guarantee of current filesystem state.
    pub fn plan(&self) -> &RemovalPlan {
        &self.plan
    }
}

/// Derive a complete read-only permanent-removal preview from current selection.
///
/// Consumes the listing so this request does not retain Windows payload reader
/// handles that deny deletion. Other clones/independent readers may still prevent
/// future apply; never close another caller's handles or force sharing bypasses.
/// Root/ancestor control and stopped skill writers remain caller obligations.
/// No locks, staging, chmod, payload execution, environment reads or network occur.
pub fn plan_removal(
    catalog: &Catalog,
    request: RemovalRequest,
) -> Result<RemovalPlan, RemovalError> {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    {
        plan::build(catalog, request)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (catalog, request);
        Err(RemovalError::UnsupportedPlatform)
    }
}
