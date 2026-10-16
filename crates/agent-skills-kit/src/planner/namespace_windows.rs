use super::*;
use crate::filesystem_windows::{self as native, Kind, reader, security};
use std::{fs, os::windows::ffi::OsStrExt, path::Component, sync::Arc};

fn error(path: &Path, operation: &'static str, error: io::Error) -> NamespaceError {
    if let Some(reason) = error
        .get_ref()
        .and_then(|cause| cause.downcast_ref::<native::UnsupportedRoot>())
    {
        return NamespaceError::new(
            path,
            NamespaceErrorKind::UnsupportedFilesystem(reason.to_string()),
        );
    }
    match error.kind() {
        io::ErrorKind::Unsupported | io::ErrorKind::NotADirectory => {
            NamespaceError::new(path, NamespaceErrorKind::UnsafeEntry)
        }
        _ => NamespaceError::io(path, operation, error),
    }
}
fn file_stamp(file: &File, path: &Path) -> Result<EntryStamp, NamespaceError> {
    native::entry_stamp(file)
        .map_err(|observed| error(path, "inspect retained Windows directory", observed))
}
fn descriptor(file: &File, path: &Path) -> Result<Vec<u8>, NamespaceError> {
    security::descriptor(file)
        .map_err(|observed| error(path, "read destination security preconditions", observed))
}
fn same_directory(a: &EntryStamp, b: &EntryStamp) -> bool {
    a.file_identity() == b.file_identity() && a.mode == b.mode
}
fn anchor_current(anchor: &DirectoryAnchor, file: &File) -> Result<bool, NamespaceError> {
    Ok(
        same_directory(&anchor.stamp, &file_stamp(file, &anchor.path)?)
            && anchor.security == descriptor(file, &anchor.path)?,
    )
}
fn changed(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory | io::ErrorKind::Unsupported
    )
}

pub(super) fn observe(root: &Path, relative: &Path) -> Result<DestinationSnapshot, NamespaceError> {
    if !root.is_absolute()
        || root.as_os_str().encode_wide().any(|unit| unit == 0)
        || root
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(NamespaceError::new(root, NamespaceErrorKind::InvalidRoot));
    }
    if root.as_os_str().encode_wide().count() * 2 > 32768 {
        return Err(NamespaceError::new(
            root,
            NamespaceErrorKind::Limit("root bytes"),
        ));
    }
    if relative.as_os_str().encode_wide().count() * 2 > 8192 {
        return Err(NamespaceError::new(
            relative,
            NamespaceErrorKind::Limit("relative bytes"),
        ));
    }
    let mut components = Vec::new();
    for part in relative.components() {
        match part {
            Component::Normal(name) if native::component(name).is_ok() => {
                components.push(name.to_owned())
            }
            Component::CurDir => (),
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
    let mut missing = Vec::new();
    let handle = loop {
        match native::open_root(&candidate) {
            Ok(file) => break Arc::new(reader::Handle::new(file)),
            Err(observed) if observed.kind() == io::ErrorKind::NotFound => {
                if missing.len() == 64 {
                    return Err(NamespaceError::new(
                        root,
                        NamespaceErrorKind::Limit("missing root component count"),
                    ));
                }
                let name = candidate
                    .file_name()
                    .ok_or_else(|| NamespaceError::new(root, NamespaceErrorKind::InvalidRoot))?
                    .to_owned();
                native::component(&name)
                    .map_err(|_| NamespaceError::new(root, NamespaceErrorKind::InvalidRoot))?;
                missing.push(name);
                if !candidate.pop() {
                    return Err(NamespaceError::new(root, NamespaceErrorKind::InvalidRoot));
                }
            }
            Err(observed) => {
                return Err(error(
                    &candidate,
                    "open Windows trusted root prefix",
                    observed,
                ));
            }
        }
    };
    reader::local_ntfs(&handle).map_err(|observed| {
        if observed.kind() == io::ErrorKind::Unsupported {
            NamespaceError::new(
                &candidate,
                NamespaceErrorKind::UnsupportedFilesystem(observed.to_string()),
            )
        } else {
            error(&candidate, "verify Windows namespace filesystem", observed)
        }
    })?;
    let original = file_stamp(&handle, &candidate)?;
    let original_security = descriptor(&handle, &candidate)?;
    let canonical = fs::canonicalize(&candidate)
        .map_err(|observed| error(&candidate, "canonicalize verified Windows prefix", observed))?;
    let check = native::open_root(&canonical)
        .map_err(|observed| error(&candidate, "verify canonical Windows prefix", observed))?;
    if !same_directory(&original, &file_stamp(&check, &candidate)?)
        || original_security != descriptor(&check, &candidate)?
    {
        return Err(NamespaceError::new(
            &candidate,
            NamespaceErrorKind::NamespaceChanged,
        ));
    }
    missing.reverse();
    let mut snapshot = DestinationSnapshot {
        requested: root.join(relative),
        requested_anchor: candidate,
        canonical_destination: canonical.clone(),
        anchors: vec![DirectoryAnchor {
            path: canonical,
            stamp: original,
            handle,
            name: None,
            security: original_security,
        }],
        missing,
    };
    if snapshot.missing.is_empty() {
        let mut remaining = components.into_iter();
        while let Some(name) = remaining.next() {
            let parent = snapshot.anchors.last().expect("retained Windows prefix");
            let path = parent.path.join(&name);
            let file = match native::child(&parent.handle, &name, Kind::Directory, false) {
                Ok(file) => Arc::new(reader::Handle::new(file)),
                Err(observed) if observed.kind() == io::ErrorKind::NotFound => {
                    snapshot.missing.push(name);
                    snapshot.missing.extend(remaining);
                    break;
                }
                Err(observed) => {
                    return Err(error(
                        &path,
                        "open Windows destination component no-follow",
                        observed,
                    ));
                }
            };
            let stamp = file_stamp(&file, &path)?;
            if stamp.device != snapshot.anchors[0].stamp.device {
                return Err(NamespaceError::new(&path, NamespaceErrorKind::CrossDevice));
            }
            let security = descriptor(&file, &path)?;
            snapshot.anchors.push(DirectoryAnchor {
                path,
                stamp,
                handle: file,
                name: Some(name),
                security,
            });
        }
    } else {
        snapshot.missing.extend(components);
    }
    let mut destination = snapshot
        .anchors
        .last()
        .expect("retained Windows prefix")
        .path
        .clone();
    for component in &snapshot.missing {
        destination.push(component);
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

pub(super) fn existing_bindings_are_current(
    snapshot: &DestinationSnapshot,
) -> Result<bool, NamespaceError> {
    let root = snapshot
        .anchors
        .first()
        .expect("private checked constructor");
    let reopened = match native::open_root(&snapshot.requested_anchor) {
        Ok(file) => file,
        Err(observed) if changed(&observed) => return Ok(false),
        Err(observed) => {
            return Err(error(
                &snapshot.requested_anchor,
                "recheck Windows root binding",
                observed,
            ));
        }
    };
    if !anchor_current(root, &reopened)? {
        return Ok(false);
    }
    for pair in snapshot.anchors.windows(2) {
        let (parent, child) = (&pair[0], &pair[1]);
        let name = child.name.as_ref().expect("non-root Windows anchor name");
        let reopened = match native::child(&parent.handle, name, Kind::Directory, false) {
            Ok(file) => file,
            Err(observed) if changed(&observed) => return Ok(false),
            Err(observed) => {
                return Err(error(
                    &child.path,
                    "recheck Windows descendant binding",
                    observed,
                ));
            }
        };
        if !anchor_current(child, &reopened)? || !anchor_current(child, &child.handle)? {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(super) fn is_current(snapshot: &DestinationSnapshot) -> Result<bool, NamespaceError> {
    if !existing_bindings_are_current(snapshot)? {
        return Ok(false);
    }
    if let Some(first) = snapshot.missing.first() {
        let parent = snapshot.anchors.last().expect("retained Windows prefix");
        match native::child(&parent.handle, first, Kind::Directory, false) {
            Ok(_) => return Ok(false),
            Err(observed) if observed.kind() == io::ErrorKind::NotFound => (),
            Err(observed) if changed(&observed) => return Ok(false),
            Err(observed) => {
                return Err(error(
                    &parent.path.join(first),
                    "recheck Windows absence",
                    observed,
                ));
            }
        }
    }
    Ok(true)
}
