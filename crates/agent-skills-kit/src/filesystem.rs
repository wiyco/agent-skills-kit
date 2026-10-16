//! Shared native read primitives for inventory and destination observations.
use crate::validation::EntryStamp;
use rustix::fs::OFlags;
use std::{
    ffi::OsStr,
    fs::{File, Metadata},
    io,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
};

pub(crate) enum FilesystemError {
    Io(io::Error),
    Unsupported(String),
}

pub(crate) fn flags() -> OFlags {
    OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::NOCTTY
}

pub(crate) fn stamp(metadata: &Metadata) -> EntryStamp {
    EntryStamp {
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.mode(),
        links: metadata.nlink(),
        length: metadata.len(),
        modified: (metadata.mtime(), metadata.mtime_nsec()),
        changed: (metadata.ctime(), metadata.ctime_nsec()),
    }
}
// Native Stat integer widths differ between Linux and macOS. Explicit casts
// match std's MetadataExt representation on both supported implementations.
#[allow(clippy::unnecessary_cast)]
pub(crate) fn binding_matches(observed: &rustix::fs::Stat, expected: &EntryStamp) -> bool {
    identity_matches(observed, expected)
        && observed.st_nlink as u64 == expected.links
        && observed.st_size as u64 == expected.length
}

#[allow(clippy::unnecessary_cast)]
pub(crate) fn identity_matches(observed: &rustix::fs::Stat, expected: &EntryStamp) -> bool {
    observed.st_dev as u64 == expected.device
        && observed.st_ino as u64 == expected.inode
        && observed.st_mode as u32 == expected.mode
}

pub(crate) fn safe_component(name: &OsStr) -> bool {
    let bytes = name.as_bytes();
    if name
        .to_str()
        .is_some_and(|text| text.chars().any(char::is_control))
    {
        return false;
    }
    if bytes.is_empty()
        || matches!(bytes, b"." | b"..")
        || matches!(bytes.last(), Some(b'.' | b' '))
        || bytes
            .iter()
            .any(|byte| *byte < 32 || *byte == 127 || b"/\\:*?\"<>|".contains(byte))
    {
        return false;
    }
    let stem = bytes.split(|byte| *byte == b'.').next().unwrap_or(bytes);
    let stem = &stem[..stem
        .iter()
        .rposition(|byte| *byte != b' ')
        .map_or(0, |index| index + 1)];
    let upper = stem.to_ascii_uppercase();
    if matches!(upper.as_slice(), b"CON" | b"PRN" | b"AUX" | b"NUL") {
        return false;
    }
    if let Some(suffix) = upper
        .strip_prefix(b"COM")
        .or_else(|| upper.strip_prefix(b"LPT"))
    {
        if matches!(
            suffix,
            b"1" | b"2"
                | b"3"
                | b"4"
                | b"5"
                | b"6"
                | b"7"
                | b"8"
                | b"9"
                | b"\xc2\xb9"
                | b"\xc2\xb2"
                | b"\xc2\xb3"
        ) {
            return false;
        }
    }
    true
}

pub(crate) fn local_filesystem(file: &File) -> Result<(), FilesystemError> {
    let stats = rustix::fs::fstatfs(file).map_err(|error| FilesystemError::Io(error.into()))?;
    #[cfg(target_os = "linux")]
    let (accepted, label) = {
        let kind = stats.f_type as u32;
        (
            [
                libc::EXT4_SUPER_MAGIC as u32,
                libc::BTRFS_SUPER_MAGIC as u32,
                libc::XFS_SUPER_MAGIC as u32,
                libc::TMPFS_MAGIC as u32,
                libc::OVERLAYFS_SUPER_MAGIC as u32,
            ]
            .contains(&kind),
            format!("Linux filesystem magic {kind:#x}"),
        )
    };
    #[cfg(target_os = "macos")]
    let (accepted, label) = {
        let bytes = stats
            .f_fstypename
            .iter()
            .map(|byte| *byte as u8)
            .take_while(|byte| *byte != 0)
            .collect::<Vec<_>>();
        let local = stats.f_flags & libc::MNT_LOCAL as u32 != 0;
        (
            local && bytes == b"apfs",
            format!("macOS filesystem {bytes:?}, local={local}"),
        )
    };
    if accepted {
        Ok(())
    } else {
        Err(FilesystemError::Unsupported(label))
    }
}
