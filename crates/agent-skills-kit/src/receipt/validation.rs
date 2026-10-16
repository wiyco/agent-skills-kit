use super::*;
use crate::catalog::Platform;
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    error::Error,
    fmt,
    io::{self, Write},
};

/// Finite receipt parsing/encoding budgets, independent of filesystem authority.
#[derive(Clone, Copy, Debug)]
pub struct ReceiptLimits {
    /// JSON bytes; default 2 MiB, ceiling 16 MiB, positive only.
    pub max_document_bytes: usize,
    /// Source payload entry records; default/ceiling 65536, positive only.
    pub max_entries: usize,
    /// Units in one native path (Unix byte/Windows u16); default/ceiling 32768.
    pub max_path_units: usize,
    /// Aggregate native path units; default 16 Mi, ceiling 64 Mi, positive only.
    pub max_total_path_units: usize,
    /// Cleanup diagnostic count; default 4096, ceiling 131072, positive only.
    pub max_diagnostics: usize,
}
impl Default for ReceiptLimits {
    fn default() -> Self {
        Self {
            max_document_bytes: 2 * 1024 * 1024,
            max_entries: 65536,
            max_path_units: 32768,
            max_total_path_units: 16 * 1024 * 1024,
            max_diagnostics: 4096,
        }
    }
}
impl ReceiptLimits {
    fn validate(self) -> Result<(), ReceiptError> {
        if (1..=16 * 1024 * 1024).contains(&self.max_document_bytes)
            && (1..=65536).contains(&self.max_entries)
            && (1..=32768).contains(&self.max_path_units)
            && (1..=64 * 1024 * 1024).contains(&self.max_total_path_units)
            && (1..=131072).contains(&self.max_diagnostics)
        {
            Ok(())
        } else {
            Err(ReceiptError::InvalidLimits)
        }
    }
}

/// Receipt codec/schema failure, without discarding an executor report.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ReceiptError {
    /// Invalid zero/excessive configured budgets.
    InvalidLimits,
    /// Finite input/output or structural budget exceeded.
    Limit(&'static str),
    /// Unsupported schema version; no best-effort interpretation.
    UnsupportedVersion(u32),
    /// Invalid document relationship or canonical encoding.
    InvalidDocument(&'static str),
    /// Malformed JSON, unknown/duplicate fields, or unknown enum values.
    Json {
        /// One-based line, or zero when the encoder reports no input position.
        line: usize,
        /// One-based column, or zero for an encoder failure.
        column: usize,
    },
    /// A native path cannot be silently decoded on this platform family.
    UnsupportedPathPlatform,
}
impl fmt::Display for ReceiptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "receipt failed: {self:?}")
    }
}
impl Error for ReceiptError {}
fn json_error(error: serde_json::Error) -> ReceiptError {
    ReceiptError::Json {
        line: error.line(),
        column: error.column(),
    }
}

/// Validated immutable receipt data. Never an install/delete capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Receipt {
    document: ReceiptDocument,
}
impl Receipt {
    /// Validate schema relationships and finite structural bounds, without I/O.
    pub fn new(document: ReceiptDocument, limits: ReceiptLimits) -> Result<Self, ReceiptError> {
        validate(&document, limits)?;
        Ok(Self { document })
    }
    /// Read-only document access. Imported records remain untrusted observations.
    pub fn document(&self) -> &ReceiptDocument {
        &self.document
    }
    /// Parse bounded original JSON bytes; reject unknown schema/fields/enums.
    pub fn from_json(bytes: &[u8], limits: ReceiptLimits) -> Result<Self, ReceiptError> {
        limits.validate()?;
        if bytes.len() > limits.max_document_bytes {
            return Err(ReceiptError::Limit("JSON bytes"));
        }
        #[derive(Deserialize)]
        struct Version {
            schema_version: u32,
        }
        let version: Version = serde_json::from_slice(bytes).map_err(json_error)?;
        if version.schema_version != RECEIPT_SCHEMA_VERSION {
            return Err(ReceiptError::UnsupportedVersion(version.schema_version));
        }
        Self::new(serde_json::from_slice(bytes).map_err(json_error)?, limits)
    }
    /// Encode stable compact JSON into a bounded caller-owned buffer.
    ///
    /// This never persists data or chooses a state directory. Callers may store
    /// bytes explicitly outside payloads; a stored receipt grants no ownership.
    pub fn to_json(&self, limits: ReceiptLimits) -> Result<Vec<u8>, ReceiptError> {
        validate(&self.document, limits)?;
        let mut writer = BoundedWriter {
            bytes: Vec::new(),
            limit: limits.max_document_bytes,
            exceeded: false,
        };
        if let Err(error) = serde_json::to_writer(&mut writer, &self.document) {
            return Err(if writer.exceeded {
                ReceiptError::Limit("JSON bytes")
            } else {
                json_error(error)
            });
        }
        Ok(writer.bytes)
    }
}
struct BoundedWriter {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}
impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit - self.bytes.len() {
            self.exceeded = true;
            return Err(io::Error::other("receipt output limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Paths {
    platform: Platform,
    limits: ReceiptLimits,
    total: usize,
}
impl Paths {
    fn check(&mut self, path: &NativePath) -> Result<(), ReceiptError> {
        path.validate(self.limits.max_path_units)?;
        if path.windows() != (self.platform == Platform::Windows) {
            return Err(ReceiptError::InvalidDocument(
                "path platform differs from receipt",
            ));
        }
        self.total = self
            .total
            .checked_add(path.units())
            .ok_or(ReceiptError::Limit("path units"))?;
        if self.total > self.limits.max_total_path_units {
            return Err(ReceiptError::Limit("path units"));
        }
        Ok(())
    }
    fn diagnostic(&mut self, diagnostic: &ReceiptDiagnostic) -> Result<(), ReceiptError> {
        if let Some(path) = &diagnostic.path {
            self.check(path)?;
        }
        if diagnostic
            .operation
            .as_ref()
            .is_some_and(|text| text.len() > 256)
        {
            return Err(ReceiptError::InvalidDocument("diagnostic operation length"));
        }
        Ok(())
    }
}
fn validate(document: &ReceiptDocument, limits: ReceiptLimits) -> Result<(), ReceiptError> {
    limits.validate()?;
    if document.schema_version != RECEIPT_SCHEMA_VERSION {
        return Err(ReceiptError::UnsupportedVersion(document.schema_version));
    }
    document.operation_id.validate()?;
    if !(1..=128).contains(&document.sources.len())
        || !(1..=256).contains(&document.targets.len())
        || !(1..=1024).contains(&document.destinations.len())
    {
        return Err(ReceiptError::Limit("source/target/destination count"));
    }
    let combinations = document.sources.len() * document.targets.len();
    if combinations > 1024 {
        return Err(ReceiptError::Limit("requested combinations"));
    }
    if document.cleanup_diagnostics.len() > limits.max_diagnostics {
        return Err(ReceiptError::Limit("cleanup diagnostics"));
    }
    let mut paths = Paths {
        platform: document.platform,
        limits,
        total: 0,
    };
    let mut roots = BTreeSet::new();
    let mut count = 0usize;
    for source in &document.sources {
        paths.check(&source.root)?;
        if !roots.insert(&source.root) {
            return Err(ReceiptError::InvalidDocument("duplicate source root"));
        }
        if !(1..=64).contains(&source.name.chars().count())
            || source.name.contains('\0')
            || source.root_mode & !0o755 != 0
        {
            return Err(ReceiptError::InvalidDocument("source name or root mode"));
        }
        let mut entries = BTreeSet::new();
        let mut manifest = false;
        for entry in &source.entries {
            count += 1;
            if count > limits.max_entries {
                return Err(ReceiptError::Limit("payload entries"));
            }
            paths.check(&entry.path)?;
            if !entry.path.relative_entry()
                || !entries.insert(&entry.path)
                || entry.mode & !0o755 != 0
            {
                return Err(ReceiptError::InvalidDocument(
                    "entry path, duplicate or mode",
                ));
            }
            match entry.kind {
                ReceiptEntryKind::File => {
                    if entry.bytes > 256 * 1024 * 1024 {
                        return Err(ReceiptError::InvalidDocument("file length"));
                    }
                    entry
                        .digest
                        .as_ref()
                        .ok_or(ReceiptError::InvalidDocument("file digest missing"))?
                        .validate()?;
                    manifest |= entry.path.manifest();
                }
                ReceiptEntryKind::Directory => {
                    if entry.bytes != 0 || entry.digest.is_some() {
                        return Err(ReceiptError::InvalidDocument("directory bytes/digest"));
                    }
                }
            }
        }
        if !manifest {
            return Err(ReceiptError::InvalidDocument(
                "source manifest entry missing",
            ));
        }
    }
    let mut targets = BTreeSet::new();
    for target in &document.targets {
        if target.len() > 4096 || !targets.insert(target) {
            return Err(ReceiptError::InvalidDocument(
                "target label length or duplicate",
            ));
        }
    }
    let mut attributions = BTreeSet::new();
    let mut attempted_paths = BTreeSet::new();
    for destination in &document.destinations {
        if let Some(path) = &destination.path {
            paths.check(path)?;
        }
        if destination.attributions.is_empty() {
            return Err(ReceiptError::InvalidDocument(
                "destination attribution missing",
            ));
        }
        for attribution in &destination.attributions {
            if attribution.source >= document.sources.len()
                || attribution.target >= document.targets.len()
                || !attributions.insert(*attribution)
            {
                return Err(ReceiptError::InvalidDocument(
                    "invalid or duplicate attribution",
                ));
            }
        }
        if (destination.status == ReceiptStatus::Failed) != destination.failure.is_some() {
            return Err(ReceiptError::InvalidDocument("failure/status mismatch"));
        }
        if destination.status != ReceiptStatus::NotAttempted {
            let path = destination
                .path
                .as_ref()
                .ok_or(ReceiptError::InvalidDocument(
                    "attempted destination path missing",
                ))?;
            if !attempted_paths.insert(path) {
                return Err(ReceiptError::InvalidDocument(
                    "duplicate attempted destination path",
                ));
            }
        }
        if document.preflight_failure.is_some() && destination.status != ReceiptStatus::NotAttempted
        {
            return Err(ReceiptError::InvalidDocument(
                "preflight failure with attempted result",
            ));
        }
        if let Some(diagnostic) = &destination.failure {
            paths.diagnostic(diagnostic)?;
        }
    }
    if attributions.len() != combinations {
        return Err(ReceiptError::InvalidDocument(
            "incomplete requested attribution",
        ));
    }
    if let Some(diagnostic) = &document.preflight_failure {
        paths.diagnostic(diagnostic)?;
    }
    if document.preflight_failure.is_some() && !document.cleanup_diagnostics.is_empty() {
        return Err(ReceiptError::InvalidDocument(
            "cleanup after failed preflight",
        ));
    }
    for diagnostic in &document.cleanup_diagnostics {
        paths.diagnostic(diagnostic)?;
    }
    Ok(())
}
