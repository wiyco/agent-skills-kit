use std::{
    fs::{self, File},
    os::unix::ffi::OsStrExt,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use super::*;
use crate::filesystem::{self, binding_matches, flags, identity_matches, safe_component, stamp};
use rustix::fs::{AtFlags, FileType, Mode, OFlags};

fn file_stamp(file: &File, path: &Path) -> Result<EntryStamp, NamespaceError> {
    file.metadata()
        .map(|metadata| stamp(&metadata))
        .map_err(|error| NamespaceError::io(path, "fstat directory", error))
}
fn same_directory(a: &EntryStamp, b: &EntryStamp) -> bool {
    a.device == b.device && a.inode == b.inode && a.mode == b.mode
}
fn changed_errno(error: rustix::io::Errno) -> bool {
    matches!(
        error,
        rustix::io::Errno::NOENT | rustix::io::Errno::NOTDIR | rustix::io::Errno::LOOP
    )
}

pub(super) fn observe(root: &Path, relative: &Path) -> Result<DestinationSnapshot, NamespaceError> {
    observe_with_hook(root, relative, &mut |_| {})
}

fn observe_with_hook(
    root: &Path,
    relative: &Path,
    before_open: &mut impl FnMut(&Path),
) -> Result<DestinationSnapshot, NamespaceError> {
    if !root.is_absolute()
        || root.as_os_str().as_bytes().contains(&0)
        || root
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(NamespaceError::new(root, NamespaceErrorKind::InvalidRoot));
    }
    if root.as_os_str().as_bytes().len() > 32768 {
        return Err(NamespaceError::new(
            root,
            NamespaceErrorKind::Limit("root bytes"),
        ));
    }
    if relative.as_os_str().as_bytes().len() > 8192 {
        return Err(NamespaceError::new(
            relative,
            NamespaceErrorKind::Limit("relative bytes"),
        ));
    }
    let mut components = Vec::new();
    for part in relative.components() {
        match part {
            Component::Normal(name) if safe_component(name) => components.push(name.to_owned()),
            Component::CurDir => {}
            _ => {
                return Err(NamespaceError::new(
                    relative,
                    NamespaceErrorKind::InvalidRelativePath,
                ));
            }
        }
        if components.len() > 64 {
            return Err(NamespaceError::new(
                relative,
                NamespaceErrorKind::Limit("relative component count"),
            ));
        }
    }
    let mut candidate: PathBuf = root
        .components()
        .filter(|part| !matches!(part, Component::CurDir))
        .collect();
    let mut root_missing = Vec::new();
    let file = loop {
        match rustix::fs::open(&candidate, flags() | OFlags::DIRECTORY, Mode::empty()) {
            Ok(fd) => break Arc::new(File::from(fd)),
            Err(rustix::io::Errno::NOENT) => {
                if root_missing.len() == 64 {
                    return Err(NamespaceError::new(
                        root,
                        NamespaceErrorKind::Limit("missing root component count"),
                    ));
                }
                let name = candidate
                    .file_name()
                    .ok_or_else(|| NamespaceError::new(root, NamespaceErrorKind::InvalidRoot))?
                    .to_owned();
                root_missing.push(name);
                if !candidate.pop() {
                    return Err(NamespaceError::new(root, NamespaceErrorKind::InvalidRoot));
                }
            }
            Err(error) if changed_errno(error) => {
                return Err(NamespaceError::new(
                    &candidate,
                    NamespaceErrorKind::UnsafeEntry,
                ));
            }
            Err(error) => {
                return Err(NamespaceError::io(
                    &candidate,
                    "open trusted root prefix no-follow",
                    error.into(),
                ));
            }
        }
    };
    filesystem::local_filesystem(&file).map_err(|error| match error {
        filesystem::FilesystemError::Io(error) => {
            NamespaceError::io(&candidate, "fstatfs trusted prefix", error)
        }
        filesystem::FilesystemError::Unsupported(label) => {
            NamespaceError::new(&candidate, NamespaceErrorKind::UnsupportedFilesystem(label))
        }
    })?;
    let root_stamp = file_stamp(&file, &candidate)?;
    let canonical = fs::canonicalize(&candidate)
        .map_err(|error| NamespaceError::io(&candidate, "canonicalize trusted prefix", error))?;
    let check = rustix::fs::open(&canonical, flags() | OFlags::DIRECTORY, Mode::empty())
        .map_err(|error| NamespaceError::io(&candidate, "verify canonical prefix", error.into()))?;
    if !same_directory(&root_stamp, &file_stamp(&File::from(check), &candidate)?) {
        return Err(NamespaceError::new(
            &candidate,
            NamespaceErrorKind::NamespaceChanged,
        ));
    }
    root_missing.reverse();
    let mut snapshot = DestinationSnapshot {
        requested: root.join(relative),
        requested_anchor: candidate,
        canonical_destination: canonical.clone(),
        anchors: vec![DirectoryAnchor {
            path: canonical,
            stamp: root_stamp,
            handle: file,
            name: None,
        }],
        missing: root_missing,
    };
    if snapshot.missing.is_empty() {
        let mut remaining = components.into_iter();
        while let Some(name) = remaining.next() {
            let parent = snapshot.anchors.last().expect("retained root exists");
            let path = parent.path.join(&name);
            let observed = match rustix::fs::statat(
                &parent.handle,
                name.as_os_str(),
                AtFlags::SYMLINK_NOFOLLOW,
            ) {
                Ok(observed) => observed,
                Err(rustix::io::Errno::NOENT) => {
                    snapshot.missing.push(name);
                    snapshot.missing.extend(remaining);
                    break;
                }
                Err(error) => {
                    return Err(NamespaceError::io(
                        &path,
                        "stat destination component no-follow",
                        error.into(),
                    ));
                }
            };
            if FileType::from_raw_mode(observed.st_mode) != FileType::Directory {
                return Err(NamespaceError::new(&path, NamespaceErrorKind::UnsafeEntry));
            }
            before_open(&path);
            let fd = rustix::fs::openat(
                &parent.handle,
                name.as_os_str(),
                flags() | OFlags::DIRECTORY,
                Mode::empty(),
            )
            .map_err(|error| {
                NamespaceError::io(&path, "open destination directory no-follow", error.into())
            })?;
            let handle = Arc::new(File::from(fd));
            let opened = file_stamp(&handle, &path)?;
            if !binding_matches(&observed, &opened) {
                return Err(NamespaceError::new(
                    &path,
                    NamespaceErrorKind::NamespaceChanged,
                ));
            }
            if opened.device != snapshot.anchors[0].stamp.device {
                return Err(NamespaceError::new(&path, NamespaceErrorKind::CrossDevice));
            }
            snapshot.anchors.push(DirectoryAnchor {
                path,
                stamp: opened,
                handle,
                name: Some(name),
            });
        }
    } else {
        snapshot.missing.extend(components);
    }
    let mut destination = snapshot
        .anchors
        .last()
        .expect("retained root exists")
        .path
        .clone();
    for name in &snapshot.missing {
        destination.push(name);
    }
    snapshot.canonical_destination = destination;
    if !is_current(&snapshot)? {
        return Err(NamespaceError::new(
            &snapshot.requested,
            NamespaceErrorKind::NamespaceChanged,
        ));
    }
    Ok(snapshot)
}

pub(super) fn is_current(snapshot: &DestinationSnapshot) -> Result<bool, NamespaceError> {
    if !existing_bindings_are_current(snapshot)? {
        return Ok(false);
    }
    if let Some(first_missing) = snapshot.missing.first() {
        let parent = snapshot.anchors.last().expect("retained root exists");
        match rustix::fs::statat(
            &parent.handle,
            first_missing.as_os_str(),
            AtFlags::SYMLINK_NOFOLLOW,
        ) {
            Ok(_) => return Ok(false),
            Err(rustix::io::Errno::NOENT) => {}
            Err(error) => {
                return Err(NamespaceError::io(
                    &parent.path.join(first_missing),
                    "recheck first absent component",
                    error.into(),
                ));
            }
        }
    }
    Ok(true)
}

pub(super) fn existing_bindings_are_current(
    snapshot: &DestinationSnapshot,
) -> Result<bool, NamespaceError> {
    let root = snapshot
        .anchors
        .first()
        .expect("only internal checked constructors");
    let current = match rustix::fs::open(
        &snapshot.requested_anchor,
        flags() | OFlags::DIRECTORY,
        Mode::empty(),
    ) {
        Ok(fd) => File::from(fd),
        Err(error) if changed_errno(error) => return Ok(false),
        Err(error) => {
            return Err(NamespaceError::io(
                &snapshot.requested_anchor,
                "recheck trusted root binding",
                error.into(),
            ));
        }
    };
    if !same_directory(
        &root.stamp,
        &file_stamp(&current, &snapshot.requested_anchor)?,
    ) {
        return Ok(false);
    }
    for pair in snapshot.anchors.windows(2) {
        let parent = &pair[0];
        let child = &pair[1];
        let name = child
            .name
            .as_ref()
            .expect("non-root anchors have a component");
        let observed =
            match rustix::fs::statat(&parent.handle, name.as_os_str(), AtFlags::SYMLINK_NOFOLLOW) {
                Ok(observed) => observed,
                Err(error) if changed_errno(error) => return Ok(false),
                Err(error) => {
                    return Err(NamespaceError::io(
                        &child.path,
                        "recheck destination binding",
                        error.into(),
                    ));
                }
            };
        if !identity_matches(&observed, &child.stamp)
            || !same_directory(&child.stamp, &file_stamp(&child.handle, &child.path)?)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::unix::fs::symlink,
        sync::atomic::{AtomicU64, Ordering},
    };

    #[test]
    fn directory_swapped_after_lookup_is_not_followed() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let mut owned = None;
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "ask-namespace-race-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => {
                    owned = Some(path);
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("exclusive race fixture: {error}"),
            }
        }
        struct Root(PathBuf);
        impl Drop for Root {
            fn drop(&mut self) {
                fs::remove_dir_all(&self.0).expect("remove owned race fixture");
            }
        }
        let root = Root(owned.expect("fixture collision budget"));
        let trusted = root.0.join("trusted");
        fs::create_dir(&trusted).unwrap();
        fs::create_dir(trusted.join("child")).unwrap();
        fs::create_dir(root.0.join("outside")).unwrap();
        fs::write(root.0.join("outside/sentinel"), b"unchanged").unwrap();
        let error = observe_with_hook(&trusted, Path::new("child/missing"), &mut |path| {
            fs::remove_dir(path).unwrap();
            symlink(root.0.join("outside"), path).unwrap();
        })
        .unwrap_err();
        assert!(
            matches!(error.kind, NamespaceErrorKind::Io { raw_os_error: Some(code), .. } if code == libc::ELOOP || code == libc::ENOTDIR)
        );
        assert_eq!(
            fs::read(root.0.join("outside/sentinel")).unwrap(),
            b"unchanged"
        );
        assert!(!root.0.join("outside/missing").exists());
    }
}
