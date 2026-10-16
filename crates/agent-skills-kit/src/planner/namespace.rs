use std::{
    error::Error,
    ffi::OsString,
    fmt, io,
    path::{Path, PathBuf},
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::{fs::File, sync::Arc};

use crate::validation::EntryStamp;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "namespace_unix.rs"]
mod unix;

/// The only successful namespace states; unsafe/non-directory entries are errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DestinationPresence {
    /// At least one component is absent; no directory has been created.
    Missing,
    /// The complete destination is an opened, verified directory.
    ExistingDirectory,
}

/// Retained directory preconditions. Fields are private to prevent fabrication.
#[derive(Clone, Debug)]
pub(super) struct DirectoryAnchor {
    pub(super) path: PathBuf,
    pub(super) stamp: EntryStamp,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(super) handle: Arc<File>,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(super) name: Option<OsString>,
}

/// Bounded read-only namespace observation, retaining every opened directory.
#[derive(Clone, Debug)]
pub struct DestinationSnapshot {
    pub(super) requested: PathBuf,
    pub(super) requested_anchor: PathBuf,
    pub(super) canonical_destination: PathBuf,
    pub(super) anchors: Vec<DirectoryAnchor>,
    pub(super) missing: Vec<OsString>,
}
impl DestinationSnapshot {
    /// Requested root joined with the caller's relative components.
    pub fn requested_path(&self) -> &Path {
        &self.requested
    }
    /// Closest existing caller-trusted root prefix actually opened.
    pub fn trusted_anchor(&self) -> &Path {
        &self.requested_anchor
    }
    /// Canonical anchor plus observed/requested components; not containment proof.
    ///
    /// Descendant labels can alias on case/normalization-insensitive filesystems;
    /// consumers must compare retained identities rather than only this spelling.
    pub fn destination(&self) -> &Path {
        &self.canonical_destination
    }
    /// Missing components after the final retained directory, in creation order.
    pub fn missing_components(&self) -> &[OsString] {
        &self.missing
    }
    /// Existing directory identities/modes, including the canonical trusted anchor.
    pub fn existing_directories(&self) -> impl Iterator<Item = (&Path, &EntryStamp)> {
        self.anchors
            .iter()
            .map(|anchor| (anchor.path.as_path(), &anchor.stamp))
    }
    /// Whether the complete path exists as a verified directory.
    pub fn presence(&self) -> DestinationPresence {
        if self.missing.is_empty() {
            DestinationPresence::ExistingDirectory
        } else {
            DestinationPresence::Missing
        }
    }
    /// Recheck selected bindings, directory identities/modes and first absence.
    ///
    /// Unrelated directory additions do not invalidate a selected binding. This
    /// does not compare final-directory contents, freeze a namespace, authorize
    /// writes or replace an executor's immediate native safety checks.
    pub fn is_current(&self) -> Result<bool, NamespaceError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            unix::is_current(self)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(NamespaceError::new(
                &self.requested,
                NamespaceErrorKind::UnsupportedPlatform,
            ))
        }
    }
}

/// Failure to observe a destination safely; no partial write intent is implied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamespaceError {
    /// Relevant requested path/component.
    pub path: PathBuf,
    /// Typed reason for refusal.
    pub kind: NamespaceErrorKind,
}
impl NamespaceError {
    pub(super) fn new(path: &Path, kind: NamespaceErrorKind) -> Self {
        Self {
            path: path.to_owned(),
            kind,
        }
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(super) fn io(path: &Path, operation: &'static str, error: io::Error) -> Self {
        Self::new(
            path,
            NamespaceErrorKind::Io {
                operation,
                kind: error.kind(),
                raw_os_error: error.raw_os_error(),
            },
        )
    }
}
impl fmt::Display for NamespaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "destination observation failed for {:?}: {:?}",
            self.path, self.kind
        )
    }
}
impl Error for NamespaceError {}

/// Native observation errors are separate from plan conflicts and host evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum NamespaceErrorKind {
    /// No reviewed native reader exists on this platform.
    UnsupportedPlatform,
    /// Root is not an explicit absolute native path without parent traversal/NUL.
    InvalidRoot,
    /// Relative path contains a root/prefix/traversal or unsafe component.
    InvalidRelativePath,
    /// A fixed observation resource budget was exceeded.
    Limit(&'static str),
    /// Existing component is a link, special entry or non-directory.
    UnsafeEntry,
    /// Root filesystem is outside the shared reader eligibility policy.
    UnsupportedFilesystem(String),
    /// A descendant crosses to another device.
    CrossDevice,
    /// Selected object/binding changed while observing it.
    NamespaceChanged,
    /// Native I/O failure, retained without guessing absence.
    Io {
        /// Operation that failed.
        operation: &'static str,
        /// Portable I/O category.
        kind: io::ErrorKind,
        /// Original native error code, when available.
        raw_os_error: Option<i32>,
    },
}

/// Observe one destination using an explicit caller-trusted root and relative path.
///
/// The root may be absent: its closest existing ancestor becomes the retained
/// trust anchor and missing root components are recorded without creation. The
/// caller must control that root/ancestor namespace. Root final aliases and all
/// untrusted descendant links are rejected; trusted ancestor aliases are recorded
/// by canonicalization. Only selected directory components are opened, never file
/// payloads. Limits: 32 KiB root bytes, 8 KiB relative bytes, 64 relative components
/// and 64 missing root components. Empty relative paths observe the root itself.
///
/// Windows fails closed before filesystem access. There are no writes, locks,
/// temporary files, environment discovery or payload execution. This observation
/// alone is not an install plan, case-collision check or proof of safe execution.
pub fn observe_destination(
    root: &Path,
    relative: &Path,
) -> Result<DestinationSnapshot, NamespaceError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        unix::observe(root, relative)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = relative;
        Err(NamespaceError::new(
            root,
            NamespaceErrorKind::UnsupportedPlatform,
        ))
    }
}
