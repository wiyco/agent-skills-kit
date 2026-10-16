//! Pure native path resolution from explicit caller context.
//!
//! Resolution does not establish filesystem containment or installation safety.
//! Planning/execution must validate actual roots, identities and no-follow access.

mod host_policy;
mod native;

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
};

use crate::catalog::{
    Catalog, ContextBase, DiscoveryTraversal, DocumentationStatus, EvidencedPathRule, PathBase,
    Platform, RuleEvidence, Scope, SupportStatus,
};

/// Explicit inputs. No field is discovered from HOME, CWD or process variables.
#[derive(Clone, Debug)]
pub struct InstallContext {
    /// Requested platform; it must match the native compilation target.
    pub platform: Platform,
    /// Explicit user home, if user-based rules are requested.
    pub user_home: Option<PathBuf>,
    /// Explicit project root, independent of working directory and Git metadata.
    pub project_root: Option<PathBuf>,
    /// Explicit application workspace, never substituted with the project root.
    pub application_workspace: Option<PathBuf>,
    /// Exact caller-supplied override names/values. Unknown names have no effect.
    pub overrides: BTreeMap<OsString, OsString>,
}

impl InstallContext {
    /// Start with an explicit platform and no implicit roots or overrides.
    pub fn new(platform: Platform) -> Self {
        Self {
            platform,
            user_home: None,
            project_root: None,
            application_workspace: None,
            overrides: BTreeMap::new(),
        }
    }
}

/// Provenance of the base actually selected for a resolved path.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PathOrigin {
    /// An explicitly supplied context root.
    Context(ContextBase),
    /// A present explicit override.
    Override(OsString),
    /// An absent override used this documented fallback context.
    Fallback {
        /// Variable that was absent.
        variable: OsString,
        /// Explicit fallback base.
        base: ContextBase,
    },
}

/// A native path with decision provenance and unchanged source evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedPath {
    /// Skills container root; a validated skill name is appended by later planning.
    pub path: PathBuf,
    /// Actual selected context/override/fallback.
    pub origin: PathOrigin,
    /// Host documentation and separate coverage dimensions.
    pub evidence: RuleEvidence,
}

/// A readable location with traversal metadata, not additional write permission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedDiscoveryLocation {
    /// Selected root path and provenance.
    pub location: ResolvedPath,
    /// Ancestors are described, not enumerated or searched by this pure resolver.
    pub traversal: DiscoveryTraversal,
}

/// Information needed to interpret a computed placement without overstating it.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum ResolutionWarning {
    /// A placement does not restrict other hosts from discovering it.
    PlacementNotExclusive,
    /// No real-host discovery test is recorded for at least one rule.
    HostDiscoveryUntested,
    /// A rule has no recorded adapter-specific resolver coverage.
    ResolverEvidenceAbsent,
    /// The host's ancestor discovery was not expanded into filesystem results.
    AncestorDiscoveryNotExpanded,
    /// A limitation from the reviewed metadata.
    DocumentedLimitation(String),
}

/// Pure destination resolution; no assertion that these directories exist.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedDestination {
    /// Canonical target ID, even when selected by alias.
    pub target_id: String,
    /// Explicit requested scope.
    pub scope: Scope,
    /// Native requested platform.
    pub platform: Platform,
    /// Preferred native write container, never substituted with a shared reader.
    pub preferred_write: ResolvedPath,
    /// Known readable location descriptors.
    pub discovery: Vec<ResolvedDiscoveryLocation>,
    /// Deterministically ordered warnings with duplicate messages removed.
    pub warnings: Vec<ResolutionWarning>,
}

/// Why a supplied root or override cannot be used by the resolver.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PathIssue {
    /// Empty values are invalid; they do not invoke fallback.
    Empty,
    /// Relative, drive-relative and drive-root-relative paths are rejected.
    Relative,
    /// NUL cannot be passed to native filesystem APIs.
    Nul,
    /// Parent traversal must not be normalized across possible links.
    ParentComponent,
    /// A component violates native naming rules.
    InvalidComponent,
    /// Device/opaque verbatim namespaces are not filesystem roots for this API.
    DeviceNamespace,
    /// A bundled host's override policy rejects this root or ambiguous spelling.
    HostOverridePolicy,
}

/// Typed resolution failure; none of these errors creates filesystem state.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResolveError {
    /// The supplied ID/alias is unknown.
    UnknownTarget(String),
    /// A scope has no definition.
    UnsupportedScope {
        /// Canonical target ID.
        target: String,
        /// Requested scope.
        scope: Scope,
    },
    /// The requested platform does not match this build; no foreign path emulation.
    ForeignPlatform {
        /// Caller platform.
        requested: Platform,
        /// This build's supported native platform, if any.
        native: Option<Platform>,
    },
    /// A scope excludes the requested platform.
    UnsupportedPlatform(Platform),
    /// The scope is unverified, import-only or unsupported.
    UnsupportedStatus(SupportStatus),
    /// Separate configuration is required before this adapter can be enabled.
    ConfigurationRequired,
    /// No write location is known; no default is inferred.
    NoWriteLocation,
    /// A required explicit context root is absent.
    MissingContext(ContextBase),
    /// An override is absent and has no documented fallback.
    MissingOverride(OsString),
    /// A supplied context path is unsafe or ambiguous.
    InvalidContext {
        /// Relevant context base.
        base: ContextBase,
        /// Native lexical problem.
        issue: PathIssue,
    },
    /// A present override is unsafe or ambiguous, even if a fallback exists.
    InvalidOverride {
        /// Exact variable name.
        variable: OsString,
        /// Native lexical problem.
        issue: PathIssue,
    },
    /// A literal rule component is invalid on the requested native platform.
    InvalidRuleComponent(String),
    /// Provenance is not reviewed; a caller cannot bypass this through a rule.
    UnreviewedRule,
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "destination resolution failed: {self:?}")
    }
}

impl Error for ResolveError {}

/// Return this build's native platform without reading the environment.
pub const fn native_platform() -> Option<Platform> {
    native::platform()
}

/// Resolve reviewed scope rules using only explicit input.
///
/// Roots need not exist, but must be absolute native paths without parent
/// traversal, NUL or unsafe native components. Present empty/relative overrides
/// are errors; only an absent override can use its documented fallback.
/// Foreign-platform emulation, filesystem probing/canonicalization, automatic
/// root discovery, configuration confirmation and directory creation are absent.
pub fn resolve(
    catalog: &Catalog,
    target: &str,
    scope: Scope,
    context: &InstallContext,
) -> Result<ResolvedDestination, ResolveError> {
    let definition = catalog
        .get(target)
        .ok_or_else(|| ResolveError::UnknownTarget(target.to_owned()))?;
    if native::platform() != Some(context.platform) {
        return Err(ResolveError::ForeignPlatform {
            requested: context.platform,
            native: native::platform(),
        });
    }
    let placement = definition
        .scopes
        .iter()
        .find(|definition| definition.scope == scope)
        .ok_or_else(|| ResolveError::UnsupportedScope {
            target: definition.id.clone(),
            scope,
        })?;
    if !placement.platforms.contains(&context.platform) {
        return Err(ResolveError::UnsupportedPlatform(context.platform));
    }
    match placement.status {
        SupportStatus::Documented => {}
        SupportStatus::ConfigurationRequired => return Err(ResolveError::ConfigurationRequired),
        status => return Err(ResolveError::UnsupportedStatus(status)),
    }
    let preferred = placement
        .preferred_write
        .as_ref()
        .ok_or(ResolveError::NoWriteLocation)?;
    let preferred_write = resolve_rule(preferred, context)?;
    host_policy::validate(definition, scope, context, &preferred_write.origin)?;
    let mut warnings = BTreeSet::from([ResolutionWarning::PlacementNotExclusive]);
    for limitation in definition.limitations.iter().chain(&placement.limitations) {
        warnings.insert(ResolutionWarning::DocumentedLimitation(limitation.clone()));
    }
    add_evidence_warnings(&preferred.evidence, &mut warnings);
    let mut discovery = Vec::new();
    for read in &placement.discovery {
        if read.traversal == DiscoveryTraversal::AncestorsToRepository {
            warnings.insert(ResolutionWarning::AncestorDiscoveryNotExpanded);
        }
        add_evidence_warnings(&read.location.evidence, &mut warnings);
        discovery.push(ResolvedDiscoveryLocation {
            location: resolve_rule(&read.location, context)?,
            traversal: read.traversal,
        });
    }
    Ok(ResolvedDestination {
        target_id: definition.id.clone(),
        scope,
        platform: context.platform,
        preferred_write,
        discovery,
        warnings: warnings.into_iter().collect(),
    })
}

/// Resolve a selection once per canonical target ID, in canonical ID order.
///
/// Repeated IDs and aliases select the same target. Distinct target IDs retain
/// separate attribution even if their physical paths coincide; filesystem
/// destination deduplication belongs to the planner. An empty selection returns
/// an empty vector. The first failing input returns an error without any partial
/// result or side effect. This does not compare installed skill names/content.
pub fn resolve_targets<'a>(
    catalog: &Catalog,
    targets: impl IntoIterator<Item = &'a str>,
    scope: Scope,
    context: &InstallContext,
) -> Result<Vec<ResolvedDestination>, ResolveError> {
    let mut destinations = BTreeMap::new();
    for target in targets {
        let definition = catalog
            .get(target)
            .ok_or_else(|| ResolveError::UnknownTarget(target.to_owned()))?;
        if !destinations.contains_key(&definition.id) {
            let destination = resolve(catalog, target, scope, context)?;
            destinations.insert(definition.id.clone(), destination);
        }
    }
    Ok(destinations.into_values().collect())
}

pub(crate) fn add_evidence_warnings(
    evidence: &RuleEvidence,
    warnings: &mut BTreeSet<ResolutionWarning>,
) {
    if evidence.discovery_tests.is_empty() {
        warnings.insert(ResolutionWarning::HostDiscoveryUntested);
    }
    if evidence.resolver_tests.is_empty() {
        warnings.insert(ResolutionWarning::ResolverEvidenceAbsent);
    }
    for limitation in &evidence.limitations {
        warnings.insert(ResolutionWarning::DocumentedLimitation(limitation.clone()));
    }
}

fn context_root(base: ContextBase, context: &InstallContext) -> Result<PathBuf, ResolveError> {
    let path = match base {
        ContextBase::Home => context.user_home.as_ref(),
        ContextBase::Project => context.project_root.as_ref(),
        ContextBase::ApplicationWorkspace => context.application_workspace.as_ref(),
    }
    .ok_or(ResolveError::MissingContext(base))?;
    native::validate_root(path).map_err(|issue| ResolveError::InvalidContext { base, issue })?;
    Ok(path.clone())
}

fn append(path: &mut PathBuf, components: &[String]) -> Result<(), ResolveError> {
    for component in components {
        if !native::valid_component(component.as_ref()) {
            return Err(ResolveError::InvalidRuleComponent(component.clone()));
        }
        path.push(component);
    }
    Ok(())
}

fn resolve_rule(
    rule: &EvidencedPathRule,
    context: &InstallContext,
) -> Result<ResolvedPath, ResolveError> {
    if rule.evidence.documentation != DocumentationStatus::Reviewed
        || rule.evidence.source.is_none()
    {
        return Err(ResolveError::UnreviewedRule);
    }
    let (mut path, origin) = match &rule.path.base {
        PathBase::Home => (
            context_root(ContextBase::Home, context)?,
            PathOrigin::Context(ContextBase::Home),
        ),
        PathBase::Project => (
            context_root(ContextBase::Project, context)?,
            PathOrigin::Context(ContextBase::Project),
        ),
        PathBase::ApplicationWorkspace => (
            context_root(ContextBase::ApplicationWorkspace, context)?,
            PathOrigin::Context(ContextBase::ApplicationWorkspace),
        ),
        PathBase::Environment { variable, fallback } => {
            let name = OsString::from(variable);
            if let Some(value) = context.overrides.get(&name) {
                let path = Path::new(value);
                native::validate_root(path).map_err(|issue| ResolveError::InvalidOverride {
                    variable: name.clone(),
                    issue,
                })?;
                (path.to_owned(), PathOrigin::Override(name))
            } else if let Some(fallback) = fallback {
                let mut path = context_root(fallback.base, context)?;
                append(&mut path, &fallback.components)?;
                (
                    path,
                    PathOrigin::Fallback {
                        variable: name,
                        base: fallback.base,
                    },
                )
            } else {
                return Err(ResolveError::MissingOverride(name));
            }
        }
    };
    append(&mut path, &rule.path.components)?;
    Ok(ResolvedPath {
        path,
        origin,
        evidence: rule.evidence.clone(),
    })
}
