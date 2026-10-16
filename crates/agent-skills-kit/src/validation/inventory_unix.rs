use std::{
    ffi::OsStr,
    fs::{self, File, Metadata},
    io::{self, Read},
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags};

use super::*;
use crate::filesystem::{binding_matches, flags, safe_component, stamp};
use crate::validation::parse_metadata;

fn local_filesystem(file: &File, path: &Path) -> Result<(), InventoryError> {
    crate::filesystem::local_filesystem(file).map_err(|error| match error {
        crate::filesystem::FilesystemError::Io(error) => InventoryError::io(path, "fstatfs", error),
        crate::filesystem::FilesystemError::Unsupported(label) => {
            InventoryError::new(path, InventoryErrorKind::UnsupportedFilesystem(label))
        }
    })
}

fn metadata(file: &File, path: &Path) -> Result<Metadata, InventoryError> {
    file.metadata()
        .map_err(|error| InventoryError::io(path, "fstat", error))
}

pub(super) fn root_is_unchanged(inventory: &SourceInventory) -> Result<bool, InventoryError> {
    Ok(stamp(&metadata(
        &inventory.root_handle,
        &inventory.requested_root,
    )?) == inventory.root_stamp)
}

pub(super) fn inspect(
    source: &Path,
    limits: InventoryLimits,
) -> Result<ValidatedSkill, InventoryError> {
    inspect_with_hook(source, limits, &mut |_, _| {})
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    BeforeOpen,
    BeforeRead,
    AfterRead,
}

fn inspect_with_hook(
    source: &Path,
    limits: InventoryLimits,
    hook: &mut impl FnMut(Phase, &Path),
) -> Result<ValidatedSkill, InventoryError> {
    if !source.is_absolute()
        || source.as_os_str().as_bytes().contains(&0)
        || source
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(InventoryError::new(source, InventoryErrorKind::InvalidRoot));
    }
    let normalized: PathBuf = source
        .components()
        .filter(|part| !matches!(part, Component::CurDir))
        .collect();
    let component = normalized
        .file_name()
        .ok_or_else(|| InventoryError::new(source, InventoryErrorKind::InvalidRoot))?;
    let name = component
        .to_str()
        .ok_or_else(|| InventoryError::new(source, InventoryErrorKind::NonUtf8SkillName))?;
    let fd = rustix::fs::open(&normalized, flags() | OFlags::DIRECTORY, Mode::empty())
        .map_err(|error| InventoryError::io(source, "open source root no-follow", error.into()))?;
    let root = Arc::new(File::from(fd));
    local_filesystem(&root, source)?;
    let root_stamp = stamp(&metadata(&root, source)?);
    let canonical = fs::canonicalize(source)
        .map_err(|error| InventoryError::io(source, "canonicalize trusted root", error))?;
    let canonical_fd = rustix::fs::open(&canonical, flags() | OFlags::DIRECTORY, Mode::empty())
        .map_err(|error| InventoryError::io(source, "verify canonical root", error.into()))?;
    if stamp(&metadata(&File::from(canonical_fd), source)?) != root_stamp {
        return Err(InventoryError::new(
            source,
            InventoryErrorKind::SourceChanged,
        ));
    }
    let mut walker = Walker {
        source,
        limits,
        device: root_stamp.device,
        entries: Vec::new(),
        observed: 0,
        total: 0,
        manifest: None,
    };
    walker.walk(&root, Path::new(""), 0, hook)?;
    let final_fd = rustix::fs::open(&normalized, flags() | OFlags::DIRECTORY, Mode::empty())
        .map_err(|error| InventoryError::io(source, "recheck source root binding", error.into()))?;
    if stamp(&metadata(&File::from(final_fd), source)?) != root_stamp {
        return Err(InventoryError::new(
            source,
            InventoryErrorKind::SourceChanged,
        ));
    }
    let manifest = walker.manifest.take().ok_or_else(|| {
        InventoryError::new(&source.join("SKILL.md"), InventoryErrorKind::MissingSkillMd)
    })?;
    let metadata = parse_metadata(&manifest, name, limits.metadata).map_err(|error| {
        InventoryError::new(
            &source.join("SKILL.md"),
            InventoryErrorKind::Metadata(error),
        )
    })?;
    walker
        .entries
        .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(ValidatedSkill {
        metadata,
        inventory: SourceInventory {
            requested_root: source.to_owned(),
            canonical_root: canonical,
            root_stamp,
            root_handle: root,
            entries: walker.entries,
            total_bytes: walker.total,
        },
    })
}

struct Walker<'a> {
    source: &'a Path,
    limits: InventoryLimits,
    device: u64,
    entries: Vec<SourceEntry>,
    observed: usize,
    total: u64,
    manifest: Option<Vec<u8>>,
}
impl Walker<'_> {
    fn error(&self, relative: &Path, kind: InventoryErrorKind) -> InventoryError {
        InventoryError::new(&self.source.join(relative), kind)
    }

    fn walk(
        &mut self,
        directory: &File,
        relative: &Path,
        depth: usize,
        hook: &mut impl FnMut(Phase, &Path),
    ) -> Result<(), InventoryError> {
        let path = self.source.join(relative);
        let before = stamp(&metadata(directory, &path)?);
        if before.device != self.device {
            return Err(self.error(relative, InventoryErrorKind::CrossDevice));
        }
        let reader = Dir::read_from(directory)
            .map_err(|error| InventoryError::io(&path, "read anchored directory", error.into()))?;
        let mut names = Vec::new();
        for entry in reader {
            let entry = entry.map_err(|error| {
                InventoryError::io(&path, "enumerate anchored directory", error.into())
            })?;
            let bytes = entry.file_name().to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            self.observed += 1;
            if self.observed > self.limits.max_entries {
                return Err(self.error(relative, InventoryErrorKind::Limit("entry count")));
            }
            names.push(OsStr::from_bytes(bytes).to_owned());
        }
        names.sort();
        for name in names {
            let child = relative.join(&name);
            let child_path = self.source.join(&child);
            if !safe_component(&name) {
                return Err(self.error(&child, InventoryErrorKind::UnsafeName));
            }
            if child.as_os_str().as_bytes().len() > self.limits.max_path_bytes {
                return Err(self.error(&child, InventoryErrorKind::Limit("relative path bytes")));
            }
            if depth + 1 > self.limits.max_depth {
                return Err(self.error(&child, InventoryErrorKind::Limit("entry depth")));
            }
            let observed =
                rustix::fs::statat(directory, name.as_os_str(), AtFlags::SYMLINK_NOFOLLOW)
                    .map_err(|error| {
                        InventoryError::io(&child_path, "stat child no-follow", error.into())
                    })?;
            let kind = match FileType::from_raw_mode(observed.st_mode) {
                FileType::Directory => SourceEntryKind::Directory,
                FileType::RegularFile => SourceEntryKind::File,
                _ => return Err(self.error(&child, InventoryErrorKind::UnsafeEntry)),
            };
            hook(Phase::BeforeOpen, &child_path);
            let open_flags = if kind == SourceEntryKind::Directory {
                flags() | OFlags::DIRECTORY
            } else {
                flags()
            };
            let fd = rustix::fs::openat(directory, name.as_os_str(), open_flags, Mode::empty())
                .map_err(|error| {
                    InventoryError::io(
                        &child_path,
                        "open child no-follow nonblocking",
                        error.into(),
                    )
                })?;
            let mut file = File::from(fd);
            let opened = metadata(&file, &child_path)?;
            let opened_kind = if opened.is_dir() {
                Some(SourceEntryKind::Directory)
            } else if opened.is_file() {
                Some(SourceEntryKind::File)
            } else {
                None
            };
            if !binding_matches(&observed, &stamp(&opened)) || opened_kind != Some(kind) {
                return Err(self.error(&child, InventoryErrorKind::SourceChanged));
            }
            if opened.dev() != self.device {
                return Err(self.error(&child, InventoryErrorKind::CrossDevice));
            }
            let before = stamp(&opened);
            if kind == SourceEntryKind::File && before.links != 1 {
                return Err(self.error(&child, InventoryErrorKind::HardLinkedFile));
            }
            let digest = if kind == SourceEntryKind::Directory {
                self.walk(&file, &child, depth + 1, hook)?;
                None
            } else {
                Some(self.read_file(&mut file, &child, &before, hook)?)
            };
            if stamp(&metadata(&file, &child_path)?) != before {
                return Err(self.error(&child, InventoryErrorKind::SourceChanged));
            }
            let binding =
                rustix::fs::statat(directory, name.as_os_str(), AtFlags::SYMLINK_NOFOLLOW)
                    .map_err(|error| {
                        InventoryError::io(&child_path, "recheck child binding", error.into())
                    })?;
            if !binding_matches(&binding, &before) {
                return Err(self.error(&child, InventoryErrorKind::SourceChanged));
            }
            self.entries.push(SourceEntry {
                relative_path: child,
                kind,
                output_mode: before.mode & 0o755,
                stamp: before,
                digest,
            });
        }
        if stamp(&metadata(directory, &path)?) != before {
            return Err(self.error(relative, InventoryErrorKind::SourceChanged));
        }
        Ok(())
    }

    fn read_file(
        &mut self,
        file: &mut File,
        relative: &Path,
        before: &EntryStamp,
        hook: &mut impl FnMut(Phase, &Path),
    ) -> Result<[u8; 32], InventoryError> {
        let path = self.source.join(relative);
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
        hook(Phase::BeforeRead, &path);
        let mut hash = blake3::Hasher::new();
        let mut bytes = Vec::new();
        let mut read = 0u64;
        let mut buffer = [0u8; 8192];
        loop {
            // Read at most one byte beyond either budget, including growing files.
            let allowed = (max - read)
                .min(self.limits.max_total_bytes - self.total)
                .saturating_add(1)
                .min(buffer.len() as u64) as usize;
            let count = match file.read(&mut buffer[..allowed]) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => result.map_err(|error| {
                    InventoryError::io(&path, "read verified regular file", error)
                })?,
            };
            if count == 0 {
                break;
            }
            read += count as u64;
            self.total += count as u64;
            if read > max {
                return Err(self.error(relative, InventoryErrorKind::Limit("file bytes")));
            }
            if self.total > self.limits.max_total_bytes {
                return Err(self.error(relative, InventoryErrorKind::Limit("total bytes")));
            }
            hash.update(&buffer[..count]);
            if manifest {
                bytes.extend_from_slice(&buffer[..count]);
            }
        }
        hook(Phase::AfterRead, &path);
        if read != before.length {
            return Err(self.error(relative, InventoryErrorKind::SourceChanged));
        }
        if manifest {
            self.manifest = Some(bytes);
        }
        Ok(*hash.finalize().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::unix::fs::symlink,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct Root(PathBuf);
    impl Root {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            for _ in 0..100 {
                let root = std::env::temp_dir().join(format!(
                    "ask-inventory-race-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&root) {
                    Ok(()) => {
                        let source = root.join("sample");
                        fs::create_dir(&source).unwrap();
                        fs::write(
                            source.join("SKILL.md"),
                            b"---\nname: sample\ndescription: inert\n---\n",
                        )
                        .unwrap();
                        fs::write(source.join("resource"), b"abc").unwrap();
                        return Self(root);
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("exclusive test root: {error}"),
                }
            }
            panic!("temporary root collision budget exhausted")
        }
        fn source(&self) -> PathBuf {
            self.0.join("sample")
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("remove owned test root");
        }
    }

    #[test]
    fn swapped_final_entry_is_not_followed() {
        let root = Root::new();
        let outside = root.0.join("outside");
        fs::write(&outside, b"outside sentinel").unwrap();
        let error = inspect_with_hook(
            &root.source(),
            InventoryLimits::default(),
            &mut |phase, path| {
                if phase == Phase::BeforeOpen && path.file_name() == Some(OsStr::new("resource")) {
                    fs::remove_file(path).unwrap();
                    symlink(&outside, path).unwrap();
                }
            },
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            InventoryErrorKind::Io {
                raw_os_error: Some(libc::ELOOP),
                ..
            }
        ));
        assert_eq!(fs::read(&outside).unwrap(), b"outside sentinel");
    }

    #[test]
    fn swapped_directory_component_is_not_followed() {
        let root = Root::new();
        let nested = root.source().join("nested");
        fs::create_dir(&nested).unwrap();
        let outside = root.0.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"unchanged").unwrap();
        let error = inspect_with_hook(
            &root.source(),
            InventoryLimits::default(),
            &mut |phase, path| {
                if phase == Phase::BeforeOpen && path == nested {
                    fs::remove_dir(path).unwrap();
                    symlink(&outside, path).unwrap();
                }
            },
        )
        .unwrap_err();
        assert!(
            matches!(error.kind, InventoryErrorKind::Io { raw_os_error: Some(code), .. } if code == libc::ELOOP || code == libc::ENOTDIR)
        );
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"unchanged");
    }

    #[test]
    fn fifo_substitution_after_stat_cannot_block_or_be_read() {
        let root = Root::new();
        let source = root.source();
        let fifo = root.0.join("fifo");
        assert!(
            std::process::Command::new("/usr/bin/mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result =
                inspect_with_hook(&source, InventoryLimits::default(), &mut |phase, path| {
                    if phase == Phase::BeforeOpen
                        && path.file_name() == Some(OsStr::new("resource"))
                    {
                        fs::remove_file(path).unwrap();
                        fs::rename(&fifo, path).unwrap();
                    }
                });
            sender.send(result).unwrap();
        });
        // A nonblocking-flag regression fails within a finite test deadline.
        let result = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("FIFO substitution must not block");
        worker.join().unwrap();
        assert_eq!(result.unwrap_err().kind, InventoryErrorKind::SourceChanged);
    }

    #[test]
    fn changed_or_growing_source_is_rejected_with_bounded_reads() {
        for grow_past_limit in [false, true] {
            let root = Root::new();
            let limits = InventoryLimits {
                max_file_bytes: 1024,
                ..InventoryLimits::default()
            };
            let error = inspect_with_hook(&root.source(), limits, &mut |phase, path| {
                if phase == Phase::BeforeRead && path.file_name() == Some(OsStr::new("resource")) {
                    fs::write(path, vec![b'x'; if grow_past_limit { 1025 } else { 6 }]).unwrap();
                }
            })
            .unwrap_err();
            assert_eq!(
                error.kind,
                if grow_past_limit {
                    InventoryErrorKind::Limit("file bytes")
                } else {
                    InventoryErrorKind::SourceChanged
                }
            );
        }
    }

    #[test]
    fn post_read_entry_replacement_and_root_namespace_changes_are_observable() {
        let root = Root::new();
        let replacement = root.0.join("replacement");
        fs::write(&replacement, b"abc").unwrap();
        let error = inspect_with_hook(
            &root.source(),
            InventoryLimits::default(),
            &mut |phase, path| {
                if phase == Phase::AfterRead && path.file_name() == Some(OsStr::new("resource")) {
                    fs::rename(&replacement, path).unwrap();
                }
            },
        )
        .unwrap_err();
        assert_eq!(error.kind, InventoryErrorKind::SourceChanged);
        let root = Root::new();
        let skill = inspect(&root.source(), InventoryLimits::default()).unwrap();
        assert!(skill.inventory().root_is_unchanged().unwrap());
        fs::write(root.source().join("new-entry"), b"x").unwrap();
        assert!(!skill.inventory().root_is_unchanged().unwrap());
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;

    #[test]
    fn portable_component_policy_has_reserved_name_boundaries() {
        for name in [
            "normal",
            "asset space",
            "日本語",
            ".hidden",
            "com0",
            "com10",
            "not-con.txt",
        ] {
            assert!(safe_component(OsStr::new(name)));
        }
        for name in [
            "",
            ".",
            "..",
            "NUL   .log",
            "COM1.log",
            "COM¹.log",
            "LPT³",
            "control\u{85}",
            "x/y",
            "x\\y",
            "x:",
            "trailing.",
            "trailing ",
        ] {
            assert!(!safe_component(OsStr::new(name)), "accepted {name:?}");
        }
    }
}
