use super::{NativePath, ReceiptError};
use crate::catalog::{Platform, Scope};
use serde::{Deserialize, Serialize};

/// Current receipt JSON schema. Unknown versions fail rather than being guessed.
pub const RECEIPT_SCHEMA_VERSION: u32 = 1;

/// Caller-selected correlation ID; not an ownership token or unique-ID service.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OperationId(String);
impl<'de> Deserialize<'de> for OperationId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
impl OperationId {
    /// Accept 1..=128 ASCII letters/digits/dot/underscore/hyphen.
    /// Callers choose an ID suitable for their own correlation requirements.
    pub fn new(value: impl Into<String>) -> Result<Self, ReceiptError> {
        let result = Self(value.into());
        result.validate()?;
        Ok(result)
    }
    /// Exact caller-selected correlation label.
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub(super) fn validate(&self) -> Result<(), ReceiptError> {
        if (1..=128).contains(&self.0.len())
            && self
                .0
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            Ok(())
        } else {
            Err(ReceiptError::InvalidDocument("operation ID"))
        }
    }
}

/// BLAKE3-256 integrity digest in canonical lowercase hexadecimal JSON text.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ContentDigest(String);
impl<'de> Deserialize<'de> for ContentDigest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let result = Self(String::deserialize(deserializer)?);
        result.validate().map_err(serde::de::Error::custom)?;
        Ok(result)
    }
}
impl ContentDigest {
    /// Encode one approved inventory digest; never a malware/trust claim.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        use std::fmt::Write;
        let mut text = String::with_capacity(64);
        for byte in bytes {
            write!(&mut text, "{byte:02x}").expect("writing into String");
        }
        Self(text)
    }
    /// Canonical hexadecimal integrity value.
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub(super) fn validate(&self) -> Result<(), ReceiptError> {
        if self.0.len() == 64
            && self
                .0
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            Ok(())
        } else {
            Err(ReceiptError::InvalidDocument("BLAKE3-256 digest"))
        }
    }
}

/// Integrity algorithm declaration; unknown algorithms are not silently accepted.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DigestAlgorithm {
    /// BLAKE3 with a 256-bit output.
    #[serde(rename = "blake3_256")]
    Blake3,
}

/// Recorded supporting entry type; receipts never permit links/special entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReceiptEntryKind {
    /// Original regular-file bytes.
    File,
    /// Supporting directory, including empty directories.
    Directory,
}

/// Intended source payload entry, without claiming installed-inode ownership.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptEntry {
    /// Exact relative native spelling, encoded losslessly.
    pub path: NativePath,
    /// Regular file or directory.
    pub kind: ReceiptEntryKind,
    /// Intended ordinary mode capped at 0755.
    pub mode: u32,
    /// File byte length; zero for a directory.
    pub bytes: u64,
    /// File integrity digest; absent for a directory.
    pub digest: Option<ContentDigest>,
}

/// One approved source snapshot shared by all destination references.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptSource {
    /// Approved source root label; not a new authorization to inspect it.
    pub root: NativePath,
    /// General validated skill name.
    pub name: String,
    /// Intended root ordinary mode capped at 0755.
    pub root_mode: u32,
    /// Complete intended entry list with original-byte digests.
    pub entries: Vec<ReceiptEntry>,
}

/// One requested source/target reference after physical destination deduplication.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptAttribution {
    /// Index into ReceiptDocument.sources.
    pub source: usize,
    /// Index into ReceiptDocument.targets.
    pub target: usize,
}

/// Stable coarse failure class; the live InstallReport retains full typed causes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReceiptFailureCode {
    /// Catalog resolution could not produce an eligible destination.
    TargetResolution,
    /// Metadata violates a selected host's separate constraints.
    HostConstraint,
    /// Existing payload differs from the approved intended payload.
    ExistingContentDiffers,
    /// Existing skill root spelling differs from the requested raw name.
    ExistingNameMismatch,
    /// Several sources request different payloads at one physical location.
    CompetingSources,
    /// Portable case/normalization keys collide.
    NameCollision,
    /// No reviewed native platform capability.
    UnsupportedPlatform,
    /// Known conflicting or unsupported preview.
    InapplicablePlan,
    /// Invalid caller-supplied limits.
    InvalidLimits,
    /// Finite operation budget exhausted.
    Limit,
    /// Approved source snapshot changed.
    SourceChanged,
    /// Source inspection failed.
    SourceInspection,
    /// Selected namespace check failed.
    Namespace,
    /// Approved destination changed.
    DestinationChanged,
    /// Existing payload inspection failed.
    DestinationInspection,
    /// Private stage ownership/content became uncertain.
    StageChanged,
    /// An unused newly created parent remains for explicit recovery.
    CreatedParentRetained,
    /// Unreviewed permission/filesystem behavior.
    Unsupported,
    /// Native I/O failure.
    Io,
}

/// Bounded observational diagnostic, not a cleanup/overwrite capability.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptDiagnostic {
    /// Stable coarse classification.
    pub code: ReceiptFailureCode,
    /// Relevant native label, when the failure has one.
    pub path: Option<NativePath>,
    /// Exact static operation label when supplied by the native backend.
    pub operation: Option<String>,
    /// Native OS error number, interpreted only for the recorded platform.
    pub os_error: Option<i32>,
}

/// Observed result, never persistent authority to delete content.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReceiptStatus {
    /// This operation observed a successful exclusive create.
    Created,
    /// Preexisting validated content, with no ownership acquired by this operation.
    Unchanged,
    /// Attempted operation failed; existing content is not claimed owned.
    Failed,
    /// This request was not attempted; no ownership claim.
    NotAttempted,
}

/// Preview classification recorded independently of actual execution outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReceiptPlanAction {
    /// Approved absent destination.
    Create,
    /// Approved preexisting identical payload, without acquired ownership.
    Unchanged,
    /// Unmanaged/different/unsafe content blocks this request.
    Conflict,
    /// Target/context/backend/host constraint blocks this request.
    Unsupported,
}

/// One physical destination and every requested attribution.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptDestination {
    /// Native path label, absent for unresolved selections.
    pub path: Option<NativePath>,
    /// Every requested source/target pair represented by this operation.
    pub attributions: Vec<ReceiptAttribution>,
    /// Original preview decision, not a claim about current filesystem state.
    pub planned_action: ReceiptPlanAction,
    /// Every known preview blocker; empty for create/unchanged previews.
    pub plan_diagnostics: Vec<ReceiptDiagnostic>,
    /// Actual outcome; unchanged never means created by this operation.
    pub status: ReceiptStatus,
    /// Present exactly for Failed status.
    pub failure: Option<ReceiptDiagnostic>,
}

/// Public JSON document data. Construct a Receipt to validate all relationships.
///
/// Imported data is untrusted; even a valid document never authorizes execution,
/// cleanup, replacement or adoption of preexisting files.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptDocument {
    /// Must equal RECEIPT_SCHEMA_VERSION.
    pub schema_version: u32,
    /// Caller-controlled correlation ID, without an ownership implication.
    pub operation_id: OperationId,
    /// Native environment where the observations were made.
    pub platform: Platform,
    /// Explicit scope of the approved request.
    pub scope: Scope,
    /// Digest interpretation for all file records.
    pub digest_algorithm: DigestAlgorithm,
    /// Deduplicated approved source snapshots.
    pub sources: Vec<ReceiptSource>,
    /// Canonical target IDs, or exact unresolved selection labels.
    pub targets: Vec<String>,
    /// Deterministically ordered physical operations.
    pub destinations: Vec<ReceiptDestination>,
    /// Whole-plan failure before destination writes.
    pub preflight_failure: Option<ReceiptDiagnostic>,
    /// Recovery observations only; names/prefixes never establish ownership.
    pub cleanup_diagnostics: Vec<ReceiptDiagnostic>,
}
