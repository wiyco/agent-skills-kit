use super::*;
use crate::{
    filesystem_windows::{self as native, Kind, Stamp, reader},
    validation::parse_metadata,
};
use std::{
    fs::{self, File},
    io::Read,
    os::windows::ffi::OsStrExt,
    sync::Arc,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_READONLY};

fn failure(path: &Path, operation: &'static str, error: io::Error) -> InventoryError {
    if let Some(reason) = error
        .get_ref()
        .and_then(|cause| cause.downcast_ref::<native::UnsupportedRoot>())
    {
        return InventoryError::new(
            path,
            InventoryErrorKind::UnsupportedFilesystem(reason.to_string()),
        );
    }
    if error.kind() == io::ErrorKind::Unsupported {
        InventoryError::new(path, InventoryErrorKind::UnsafeEntry)
    } else {
        InventoryError::io(path, operation, error)
    }
}
fn snapshot(native: Stamp) -> EntryStamp {
    let mode = if native.kind == Kind::Directory {
        0o040555
    } else {
        0o100444
    } | if native.attributes & FILE_ATTRIBUTE_READONLY == 0 {
        0o200
    } else {
        0
    };
    EntryStamp {
        device: native.volume,
        inode: u64::from_le_bytes(native.identity[..8].try_into().unwrap()),
        mode,
        links: u64::from(native.links),
        length: native.length as u64,
        modified: (
            native.modified / 10_000_000,
            (native.modified % 10_000_000) * 100,
        ),
        changed: (
            native.changed / 10_000_000,
            (native.changed % 10_000_000) * 100,
        ),
        windows: native,
    }
}
fn observe(file: &File, path: &Path) -> Result<EntryStamp, InventoryError> {
    reader::unnamed_stream_only(file)
        .map_err(|error| failure(path, "reject alternate Windows data streams", error))?;
    native::stamp(file)
        .map(snapshot)
        .map_err(|error| failure(path, "inspect retained Windows object", error))
}
fn local(file: &File, path: &Path) -> Result<(), InventoryError> {
    reader::local_ntfs(file).map_err(|error| {
        if error.kind() == io::ErrorKind::Unsupported {
            InventoryError::new(
                path,
                InventoryErrorKind::UnsupportedFilesystem(error.to_string()),
            )
        } else {
            InventoryError::io(path, "verify local NTFS eligibility", error)
        }
    })
}
pub(super) fn root_is_unchanged(inventory: &SourceInventory) -> Result<bool, InventoryError> {
    Ok(observe(&inventory.root_handle, &inventory.requested_root)? == inventory.root_stamp)
}
pub(super) fn inspect(
    source: &Path,
    limits: InventoryLimits,
) -> Result<ValidatedSkill, InventoryError> {
    if !source.is_absolute()
        || source
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
        || source.as_os_str().encode_wide().any(|unit| unit == 0)
    {
        return Err(InventoryError::new(source, InventoryErrorKind::InvalidRoot));
    }
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| InventoryError::new(source, InventoryErrorKind::NonUtf8SkillName))?;
    let root = Arc::new(reader::Handle::new(native::open_root(source).map_err(
        |error| {
            failure(
                source,
                "open Windows source root without reparse traversal",
                error,
            )
        },
    )?));
    local(&root, source)?;
    let canonical = fs::canonicalize(source).map_err(|error| {
        InventoryError::io(source, "canonicalize caller-trusted Windows root", error)
    })?;
    let canonical_handle = native::open_root(&canonical)
        .map_err(|error| failure(source, "verify canonical Windows root", error))?;
    let original = observe(&root, source)?;
    if observe(&canonical_handle, source)? != original {
        return Err(InventoryError::new(
            source,
            InventoryErrorKind::SourceChanged,
        ));
    }
    let mut validated = inspect_retained(root, source, name, limits)?;
    let reopened = native::open_root(source)
        .map_err(|error| failure(source, "recheck Windows root binding", error))?;
    if observe(&reopened, source)? != original || validated.inventory.root_stamp != original {
        return Err(InventoryError::new(
            source,
            InventoryErrorKind::SourceChanged,
        ));
    }
    validated.inventory.canonical_root = canonical;
    Ok(validated)
}
fn inspect_retained(
    root: Arc<reader::Handle>,
    label: &Path,
    name: &str,
    limits: InventoryLimits,
) -> Result<ValidatedSkill, InventoryError> {
    local(&root, label)?;
    let original = observe(&root, label)?;
    let mut walker = Walker {
        label,
        limits,
        volume: original.device,
        entries: vec![],
        total: 0,
        manifest: None,
        observed: 0,
    };
    walker.walk(&root, Path::new(""), 0)?;
    if observe(&root, label)? != original {
        return Err(InventoryError::new(
            label,
            InventoryErrorKind::SourceChanged,
        ));
    }
    let manifest = walker.manifest.take().ok_or_else(|| {
        InventoryError::new(&label.join("SKILL.md"), InventoryErrorKind::MissingSkillMd)
    })?;
    let metadata = parse_metadata(&manifest, name, limits.metadata).map_err(|diagnostic| {
        InventoryError::new(
            &label.join("SKILL.md"),
            InventoryErrorKind::Metadata(diagnostic),
        )
    })?;
    walker
        .entries
        .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(ValidatedSkill {
        metadata,
        inventory: SourceInventory {
            requested_root: label.into(),
            canonical_root: label.into(),
            root_stamp: original,
            root_handle: root,
            entries: walker.entries,
            total_bytes: walker.total,
        },
    })
}
struct Walker<'a> {
    label: &'a Path,
    limits: InventoryLimits,
    volume: u64,
    entries: Vec<SourceEntry>,
    total: u64,
    manifest: Option<Vec<u8>>,
    observed: usize,
}
impl Walker<'_> {
    fn error(&self, relative: &Path, kind: InventoryErrorKind) -> InventoryError {
        InventoryError::new(&self.label.join(relative), kind)
    }
    fn walk(
        &mut self,
        directory: &reader::Handle,
        relative: &Path,
        depth: usize,
    ) -> Result<(), InventoryError> {
        let path = self.label.join(relative);
        let before = observe(directory, &path)?;
        if before.device != self.volume {
            return Err(self.error(relative, InventoryErrorKind::CrossDevice));
        }
        let names = directory
            .entries(self.limits.max_entries - self.observed)
            .map_err(|error| {
                if error.kind() == io::ErrorKind::FileTooLarge {
                    self.error(relative, InventoryErrorKind::Limit("entry count"))
                } else {
                    failure(&path, "enumerate retained Windows directory", error)
                }
            })?;
        self.observed += names.len();
        for entry in names {
            let child = relative.join(&entry.name);
            let path = self.label.join(&child);
            native::component(&entry.name)
                .map_err(|_| self.error(&child, InventoryErrorKind::UnsafeName))?;
            if child.as_os_str().encode_wide().count() * 2 > self.limits.max_path_bytes {
                return Err(self.error(&child, InventoryErrorKind::Limit("relative path bytes")));
            }
            if depth + 1 > self.limits.max_depth {
                return Err(self.error(&child, InventoryErrorKind::Limit("entry depth")));
            }
            if native::unsupported_attributes(entry.attributes) {
                return Err(self.error(&child, InventoryErrorKind::UnsafeEntry));
            }
            let kind = if entry.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                Kind::Directory
            } else {
                Kind::File
            };
            let mut opened = reader::Handle::new(
                native::child(directory, &entry.name, kind, false)
                    .map_err(|error| failure(&path, "open Windows child no-follow", error))?,
            );
            let observed = observe(&opened, &path)?;
            if observed.device != self.volume {
                return Err(self.error(&child, InventoryErrorKind::CrossDevice));
            }
            let digest = if kind == Kind::Directory {
                self.walk(&opened, &child, depth + 1)?;
                None
            } else {
                Some(self.read(&mut opened, &child, &observed)?)
            };
            if observe(&opened, &path)? != observed {
                return Err(self.error(&child, InventoryErrorKind::SourceChanged));
            }
            // The child denies delete sharing while inspected. Reopen by retained
            // parent anyway and compare the full native identity/stamp.
            let rebound = native::child(directory, &entry.name, kind, false)
                .map_err(|error| failure(&path, "recheck Windows child binding", error))?;
            if observe(&rebound, &path)? != observed {
                return Err(self.error(&child, InventoryErrorKind::SourceChanged));
            }
            self.entries.push(SourceEntry {
                relative_path: child,
                kind: if kind == Kind::Directory {
                    SourceEntryKind::Directory
                } else {
                    SourceEntryKind::File
                },
                output_mode: observed.mode & 0o755,
                stamp: observed,
                digest,
            });
        }
        if observe(directory, &path)? != before {
            return Err(self.error(relative, InventoryErrorKind::SourceChanged));
        }
        Ok(())
    }
    fn read(
        &mut self,
        file: &mut File,
        relative: &Path,
        before: &EntryStamp,
    ) -> Result<[u8; 32], InventoryError> {
        let manifest = relative == Path::new("SKILL.md");
        let max = if manifest {
            self.limits
                .max_file_bytes
                .min(self.limits.metadata.max_document_bytes as u64)
        } else {
            self.limits.max_file_bytes
        };
        if before.length > max {
            return Err(self.error(relative, InventoryErrorKind::Limit("file bytes")));
        }
        if before.length > self.limits.max_total_bytes - self.total {
            return Err(self.error(relative, InventoryErrorKind::Limit("total bytes")));
        }
        let mut hash = blake3::Hasher::new();
        let mut bytes = vec![];
        let mut count = 0u64;
        let mut buffer = [0u8; 8192];
        loop {
            let allowed = (max - count)
                .min(self.limits.max_total_bytes - self.total)
                .saturating_add(1)
                .min(buffer.len() as u64) as usize;
            let read = match file.read(&mut buffer[..allowed]) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => result.map_err(|error| {
                    InventoryError::io(
                        &self.label.join(relative),
                        "read verified Windows regular file",
                        error,
                    )
                })?,
            };
            if read == 0 {
                break;
            }
            count += read as u64;
            self.total += read as u64;
            if count > max {
                return Err(self.error(relative, InventoryErrorKind::Limit("file bytes")));
            }
            if self.total > self.limits.max_total_bytes {
                return Err(self.error(relative, InventoryErrorKind::Limit("total bytes")));
            }
            hash.update(&buffer[..read]);
            if manifest {
                bytes.extend_from_slice(&buffer[..read]);
            }
        }
        if count != before.length {
            return Err(self.error(relative, InventoryErrorKind::SourceChanged));
        }
        if manifest {
            self.manifest = Some(bytes);
        }
        Ok(*hash.finalize().as_bytes())
    }
}
