//! Serializable data model. Validation occurs when constructing a catalog.

use serde::{Deserialize, Serialize};

/// Explicit installation context; no implicit current-directory or home lookup.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Scope {
    /// A caller-selected project root.
    Project,
    /// A caller-selected user environment.
    User,
    /// A caller-selected application workspace, distinct from a project.
    ApplicationWorkspace,
}

/// Native platform applicability, independent of actual test evidence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Platform {
    /// Linux.
    Linux,
    /// macOS.
    Macos,
    /// Windows.
    Windows,
}

/// Documentation/support classification, not a promise of tested installation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SupportStatus {
    /// Reviewed host documentation describes local placement.
    Documented,
    /// Placement requires separately documented configuration.
    ConfigurationRequired,
    /// Only host-managed import is known; no local write destination.
    ImportOnly,
    /// Local installation is explicitly unsupported.
    Unsupported,
    /// Evidence is incomplete; do not advertise installability.
    Unverified,
}

/// A base supplied explicitly by the caller.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ContextBase {
    /// User home.
    Home,
    /// Project root.
    Project,
    /// Application workspace root.
    ApplicationWorkspace,
}

/// A documented fallback for an absent environment override.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackPath {
    /// Explicit context base.
    pub base: ContextBase,
    /// Literal relative components, without interpolation.
    pub components: Vec<String>,
}

/// Interpretable path base; environment syntax is never embedded in a string.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[non_exhaustive]
pub enum PathBase {
    /// Caller home.
    Home,
    /// Caller project root.
    Project,
    /// Caller application workspace.
    ApplicationWorkspace,
    /// Caller-supplied environment value, not a process-global read.
    Environment {
        /// Exact variable name, interpreted by a future resolver.
        variable: String,
        /// Explicit fallback; absent means missing context, never a guessed path.
        fallback: Option<FallbackPath>,
    },
}

/// A relative path appended to an explicit base.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PathRule {
    /// Context or environment base.
    pub base: PathBase,
    /// Literal path components. Empty means the explicitly chosen base itself.
    pub components: Vec<String>,
}

/// Current state of documentation review, separate from test coverage.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DocumentationStatus {
    /// The cited section was reviewed.
    Reviewed,
    /// Review is incomplete.
    Unverified,
    /// Primary sources disagree.
    Conflicting,
    /// The record is known to be stale.
    Outdated,
}

/// Host-owned documentation provenance for one rule.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRecord {
    /// Resolved HTTPS documentation URL, not proof of authority by itself.
    pub url: String,
    /// Relevant section.
    pub section: String,
    /// Review date in Gregorian YYYY-MM-DD format.
    pub accessed: String,
    /// Product/version applicability, including explicit uncertainty.
    pub applicability: String,
}

/// An actual host-discovery test record; schema tests must not populate this.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryTest {
    /// Native tested platform.
    pub platform: Platform,
    /// Actual tested host version.
    pub host_version: String,
    /// Evidence/report reference.
    pub evidence: String,
}

/// Independent evidence dimensions for a path rule.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuleEvidence {
    /// Documentation review state.
    pub documentation: DocumentationStatus,
    /// Missing evidence remains explicit for unverified candidates.
    pub source: Option<EvidenceRecord>,
    /// Actual resolver tests, not schema-validation tests.
    pub resolver_tests: Vec<String>,
    /// Actual real-host discovery runs.
    pub discovery_tests: Vec<DiscoveryTest>,
    /// Known rule-specific limitations and evidence gaps.
    pub limitations: Vec<String>,
}

/// A path and its own provenance; evidence is not inherited from another host.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidencedPathRule {
    /// Explicit path expression.
    pub path: PathRule,
    /// Rule-specific documentation and tests.
    pub evidence: RuleEvidence,
}

/// Host discovery behavior, separate from the preferred write location.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoveryTraversal {
    /// Only the selected root is described.
    Exact,
    /// The host searches ancestors bounded by its repository/worktree semantics.
    AncestorsToRepository,
}

/// A readable host location; listing it does not authorize writes there.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryLocation {
    /// Read location and evidence.
    pub location: EvidencedPathRule,
    /// Documented traversal semantics.
    pub traversal: DiscoveryTraversal,
}

/// Placement and discovery facts for one scope/platform set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeDefinition {
    /// Caller context required.
    pub scope: Scope,
    /// Documentation classification.
    pub status: SupportStatus,
    /// Applicable platforms; actual test coverage remains separate.
    pub platforms: Vec<Platform>,
    /// No value means no write destination; it never selects a default.
    pub preferred_write: Option<EvidencedPathRule>,
    /// Documented readable locations, including compatibility locations.
    pub discovery: Vec<DiscoveryLocation>,
    /// Scope-specific conditions, configuration requirements and uncertainty.
    pub limitations: Vec<String>,
}

/// One product variant with a stable identifier and explicit aliases.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetDefinition {
    /// Stable lowercase identifier; never reused for a different meaning.
    pub id: String,
    /// Additional identifiers in the same global namespace.
    pub aliases: Vec<String>,
    /// Stable product identity, separate from its variant.
    pub product_id: String,
    /// CLI, IDE, cloud or other explicitly distinguished variant.
    pub variant: String,
    /// Scope definitions; absent scopes are not supported implicitly.
    pub scopes: Vec<ScopeDefinition>,
    /// Product-level limitations.
    pub limitations: Vec<String>,
}

/// Wire document. Pass through [`super::Catalog::new`] before use.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogDocument {
    /// Explicit format version; unsupported versions are rejected.
    pub schema_version: u32,
    /// Definitions; validated catalog iteration is sorted by ID.
    pub targets: Vec<TargetDefinition>,
}
