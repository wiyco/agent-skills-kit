use std::{
    error::Error,
    fmt, io,
    path::{Path, PathBuf},
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::fs::File;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
use std::sync::Arc;

use super::{Diagnostic, MetadataLimits, SkillMetadata};

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "inventory_unix.rs"]
mod unix;

#[cfg(windows)]
#[path = "inventory_windows.rs"]
mod windows;

/// Finite inventory budgets; no entries are silently excluded.
#[derive(Clone, Copy, Debug)]
pub struct InventoryLimits {
    /// Files and directories below the source root; default 4096, ceiling 65536.
    pub max_entries: usize,
    /// Bytes per regular file; default 8 MiB, ceiling 256 MiB.
    pub max_file_bytes: u64,
    /// Total regular-file bytes; default 64 MiB, ceiling 1 GiB.
    pub max_total_bytes: u64,
    /// Relative component depth; default 32, ceiling 64.
    pub max_depth: usize,
    /// Relative native path bytes; default 4096, ceiling 8192.
    pub max_path_bytes: usize,
    /// Additional SKILL.md parsing budgets.
    pub metadata: MetadataLimits,
}
impl Default for InventoryLimits {
    fn default() -> Self {
        Self {
            max_entries: 4096,
            max_file_bytes: 8 * 1024 * 1024,
            max_total_bytes: 64 * 1024 * 1024,
            max_depth: 32,
            max_path_bytes: 4096,
            metadata: MetadataLimits::default(),
        }
    }
}
impl InventoryLimits {
    pub(crate) fn validate(self) -> bool {
        (1..=65536).contains(&self.max_entries)
            && (1..=256 * 1024 * 1024).contains(&self.max_file_bytes)
            && (1..=1024 * 1024 * 1024).contains(&self.max_total_bytes)
            && (1..=64).contains(&self.max_depth)
            && (1..=8192).contains(&self.max_path_bytes)
            && self.metadata.validate().is_ok()
    }
}

/// Native identity and mutation stamp, excluding access-time bookkeeping.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntryStamp {
    #[cfg(windows)]
    pub(crate) windows: crate::filesystem_windows::Stamp,
    /// Native device identifier (not a cross-machine persistent identifier).
    pub device: u64,
    /// Native inode identifier on Unix; low 64 identity bits for Windows diagnostics.
    /// Windows security comparisons retain the full identifier internally.
    pub inode: u64,
    /// Full observed Unix mode, including type and privilege bits for revalidation.
    /// Windows records ordinary read-only intent, not a Unix ACL equivalent.
    pub mode: u32,
    /// Observed hard-link count.
    pub links: u64,
    /// Observed native length; directory length is not payload size.
    pub length: u64,
    /// Modification seconds/nanoseconds.
    pub modified: (i64, i64),
    /// Metadata-change seconds/nanoseconds.
    pub changed: (i64, i64),
}
impl EntryStamp {
    /// Volume/device and complete native object identifier for this observation.
    ///
    /// Unix inode values are zero-extended; Windows retains all 128 file-ID bits.
    /// This is an opaque, lifetime-scoped identity, not a persistent ownership token.
    pub fn file_identity(&self) -> (u64, [u8; 16]) {
        #[cfg(windows)]
        let identifier = self.windows.identity;
        #[cfg(not(windows))]
        let identifier = u128::from(self.inode).to_le_bytes();
        (self.device, identifier)
    }
}

/// Only these source entry types can enter a successful inventory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SourceEntryKind {
    /// Directory, including empty resource directories.
    Directory,
    /// Singly linked regular file.
    File,
}

/// Observed source entry. This record grants no write authorization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceEntry {
    /// Native relative identity; non-UTF-8 resource names are retained on Unix.
    pub relative_path: PathBuf,
    /// Verified opened-object type.
    pub kind: SourceEntryKind,
    /// Identity and mutation precondition for later revalidation.
    pub stamp: EntryStamp,
    /// Payload mode intent: source mode capped at 0755, without adding permissions.
    pub output_mode: u32,
    /// BLAKE3-256 digest for files; absent for directories.
    pub digest: Option<[u8; 32]>,
}

/// Complete immutable inventory from one successful bounded inspection.
#[derive(Clone, Debug)]
pub struct SourceInventory {
    pub(super) requested_root: PathBuf,
    pub(super) canonical_root: PathBuf,
    pub(super) root_stamp: EntryStamp,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(super) root_handle: Arc<File>,
    #[cfg(windows)]
    pub(super) root_handle: Arc<crate::filesystem_windows::reader::Handle>,
    pub(super) entries: Vec<SourceEntry>,
    pub(super) total_bytes: u64,
}
impl SourceInventory {
    /// Original explicit root; its ancestor namespace must remain caller-controlled.
    pub fn requested_root(&self) -> &Path {
        &self.requested_root
    }
    /// Canonical root recorded at the caller's trust boundary, not descendant permission.
    pub fn canonical_root(&self) -> &Path {
        &self.canonical_root
    }
    /// Root identity/preconditions observed through the retained handle.
    pub fn root_stamp(&self) -> &EntryStamp {
        &self.root_stamp
    }
    /// Deterministically ordered relative entries, with no hidden exclusions.
    pub fn entries(&self) -> &[SourceEntry] {
        &self.entries
    }
    /// Total bytes actually read from all regular files.
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(crate) fn retained_root(&self) -> &Arc<File> {
        &self.root_handle
    }

    /// Re-observe the retained root without resolving its pathname.
    ///
    /// Later planning must also revalidate pathname identity and every descendant;
    /// this check alone does not prove a complete source tree is unchanged.
    pub fn root_is_unchanged(&self) -> Result<bool, InventoryError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            unix::root_is_unchanged(self)
        }
        #[cfg(windows)]
        {
            windows::root_is_unchanged(self)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            Err(InventoryError::new(
                &self.requested_root,
                InventoryErrorKind::UnsupportedPlatform,
            ))
        }
    }
}

/// General metadata plus a complete source inventory; host validation is separate.
#[derive(Clone, Debug)]
pub struct ValidatedSkill {
    pub(super) metadata: SkillMetadata,
    pub(super) inventory: SourceInventory,
}
impl ValidatedSkill {
    /// General-spec metadata with exact original manifest bytes.
    pub fn metadata(&self) -> &SkillMetadata {
        &self.metadata
    }
    /// Resource entries and mutation preconditions; not a copy plan.
    pub fn inventory(&self) -> &SourceInventory {
        &self.inventory
    }
}

/// A fail-fast inspection error. No partial inventory is returned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InventoryError {
    /// Entry or directory that failed. Subsequent entries were not inspected.
    pub path: PathBuf,
    /// Typed failure; no rejected entry is silently omitted from a success result.
    pub kind: InventoryErrorKind,
}
impl InventoryError {
    pub(super) fn new(path: &Path, kind: InventoryErrorKind) -> Self {
        Self {
            path: path.to_owned(),
            kind,
        }
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub(super) fn io(path: &Path, operation: &'static str, error: io::Error) -> Self {
        Self::new(
            path,
            InventoryErrorKind::Io {
                operation,
                kind: error.kind(),
                raw_os_error: error.raw_os_error(),
            },
        )
    }
}
impl fmt::Display for InventoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "source inventory failed for {:?}: {:?}",
            self.path, self.kind
        )
    }
}
impl Error for InventoryError {}

/// Filesystem failures are distinct from general metadata and host constraints.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum InventoryErrorKind {
    /// No reviewed native reader is available on this platform.
    UnsupportedPlatform,
    /// A configured budget is zero or above its hard ceiling.
    InvalidLimits,
    /// Root must be an explicit native absolute path without parent traversal/NUL.
    InvalidRoot,
    /// Source root basename cannot be represented as UTF-8 metadata identity.
    NonUtf8SkillName,
    /// Root filesystem is outside the reviewed local-filesystem allowlist.
    UnsupportedFilesystem(String),
    /// A descendant crosses to another device/mount; not traversed.
    CrossDevice,
    /// A resource name violates the portable component policy.
    UnsafeName,
    /// A symlink or special entry was rejected without reading its payload.
    UnsafeEntry,
    /// A regular file has multiple hard links.
    HardLinkedFile,
    /// Opened identity, type, metadata or observed length changed during inspection.
    SourceChanged,
    /// A finite named inventory budget was exceeded.
    Limit(&'static str),
    /// No root entry named exactly SKILL.md exists as a regular file.
    MissingSkillMd,
    /// Manifest validation failed; its original location is retained.
    Metadata(Diagnostic),
    /// Native I/O failure with reproducible operation/kind/OS code.
    Io {
        /// Operation that failed.
        operation: &'static str,
        /// Portable I/O classification.
        kind: io::ErrorKind,
        /// Native OS error code, when present.
        raw_os_error: Option<i32>,
    },
}

/// Inspect an explicitly approved local source using bounded, anchored reads.
///
/// The caller must control the root and its ancestors for the operation. Final
/// root aliases are rejected; canonicalization of trusted ancestors is recorded.
/// Descendant links/special/multiply linked files and device crossings fail closed.
/// Only reviewed local Linux/macOS candidates and local Windows NTFS are eligible.
/// Windows inventory is read-only and does not enable copy execution. Other
/// platforms return unsupported before filesystem access.
///
/// No writes, environment mutation or content execution occur. This is not an
/// atomic snapshot of an arbitrarily mutating tree or an installation guarantee.
/// Errors identify the first failure; remaining entries are not inspected.
pub fn inventory_skill(
    source: &Path,
    limits: InventoryLimits,
) -> Result<ValidatedSkill, InventoryError> {
    if !limits.validate() {
        return Err(InventoryError::new(
            source,
            InventoryErrorKind::InvalidLimits,
        ));
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        unix::inspect(source, limits)
    }
    #[cfg(windows)]
    {
        windows::inspect(source, limits)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Err(InventoryError::new(
            source,
            InventoryErrorKind::UnsupportedPlatform,
        ))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn inventory_retained(
    root: Arc<File>,
    label: &Path,
    name: &str,
    limits: InventoryLimits,
) -> Result<ValidatedSkill, InventoryError> {
    if !limits.validate() {
        return Err(InventoryError::new(
            label,
            InventoryErrorKind::InvalidLimits,
        ));
    }
    unix::inspect_retained(root, label, name, limits)
}
