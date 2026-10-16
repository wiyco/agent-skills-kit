//! Retained-handle primitives, exercised natively before backend integration.
use std::{
    ffi::OsStr,
    fs::{File, OpenOptions},
    io,
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::Path,
    ptr,
};
use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN,
            FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
        },
    },
    Win32::{
        Foundation::{OBJ_CASE_INSENSITIVE, RtlNtStatusToDosError, UNICODE_STRING},
        Storage::FileSystem::*,
        System::WindowsProgramming::DRIVE_FIXED,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    Directory,
    File,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Stamp {
    pub(crate) volume: u64,
    pub(crate) identity: [u8; 16],
    pub(crate) attributes: u32,
    pub(crate) links: u32,
    pub(crate) length: i64,
    pub(crate) modified: i64,
    pub(crate) changed: i64,
    pub(crate) kind: Kind,
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn unsupported(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message)
}
#[derive(Debug)]
pub(crate) struct UnsupportedRoot(pub(crate) &'static str);
impl std::fmt::Display for UnsupportedRoot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}
impl std::error::Error for UnsupportedRoot {}
fn unsupported_root(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, UnsupportedRoot(message))
}

pub(crate) fn component(name: &OsStr) -> io::Result<Vec<u16>> {
    let wide = name.encode_wide().collect::<Vec<_>>();
    if name
        .to_str()
        .is_some_and(|text| text.chars().any(char::is_control))
        || wide.is_empty()
        || wide.len() > 255
        || wide == [46]
        || wide == [46, 46]
        || matches!(wide.last(), Some(32 | 46))
        || wide.iter().any(|unit| {
            *unit < 32
                || (127..=159).contains(unit)
                || [47, 92, 58, 42, 63, 34, 60, 62, 124].contains(unit)
        })
    {
        return Err(invalid("unsafe native component"));
    }
    let stem = wide.split(|unit| *unit == 46).next().unwrap();
    let stem = &stem[..stem
        .iter()
        .rposition(|unit| *unit != 32)
        .map_or(0, |index| index + 1)];
    let upper = stem
        .iter()
        .map(|unit| {
            if (97..=122).contains(unit) {
                unit - 32
            } else {
                *unit
            }
        })
        .collect::<Vec<_>>();
    if ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|name| upper == name.encode_utf16().collect::<Vec<_>>())
        || (upper.len() == 4
            && (upper[..3] == [67, 79, 77] || upper[..3] == [76, 80, 84])
            && ((49..=57).contains(&upper[3]) || [0xb9, 0xb2, 0xb3].contains(&upper[3])))
    {
        return Err(invalid("reserved native component"));
    }
    Ok(wide)
}

fn information<T: Default>(file: &File, class: FILE_INFO_BY_HANDLE_CLASS) -> io::Result<T> {
    let mut value = T::default();
    // SAFETY: private call sites pair each SDK class with its exact SDK struct;
    // value is writable/aligned for its full size and the retained handle is live.
    let result = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            class,
            (&mut value as *mut T).cast(),
            size_of::<T>() as u32,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(value)
    }
}

pub(crate) fn unsupported_attributes(attributes: u32) -> bool {
    attributes
        & (FILE_ATTRIBUTE_REPARSE_POINT
            | FILE_ATTRIBUTE_DEVICE
            | FILE_ATTRIBUTE_OFFLINE
            | FILE_ATTRIBUTE_ENCRYPTED
            | FILE_ATTRIBUTE_RECALL_ON_OPEN
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
        != 0
}

pub(crate) fn stamp(file: &File) -> io::Result<Stamp> {
    // SAFETY: only inspects the live retained handle and writes no memory.
    if unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_DISK {
        return Err(unsupported("not a disk object"));
    }
    let basic: FILE_BASIC_INFO = information(file, FileBasicInfo)?;
    let tag: FILE_ATTRIBUTE_TAG_INFO = information(file, FileAttributeTagInfo)?;
    if unsupported_attributes(tag.FileAttributes)
        || unsupported_attributes(basic.FileAttributes)
        || tag.ReparseTag != 0
    {
        return Err(unsupported(
            "reparse, device, offline, recall or encrypted object rejected",
        ));
    }
    let standard: FILE_STANDARD_INFO = information(file, FileStandardInfo)?;
    if standard.DeletePending || standard.EndOfFile < 0 {
        return Err(unsupported("pending deletion or invalid length"));
    }
    if !standard.Directory && standard.NumberOfLinks != 1 {
        return Err(unsupported("multiply linked file rejected"));
    }
    let id: FILE_ID_INFO = information(file, FileIdInfo)?;
    Ok(Stamp {
        volume: id.VolumeSerialNumber,
        identity: id.FileId.Identifier,
        attributes: basic.FileAttributes,
        links: standard.NumberOfLinks,
        length: standard.EndOfFile,
        modified: basic.LastWriteTime,
        changed: basic.ChangeTime,
        kind: if standard.Directory {
            Kind::Directory
        } else {
            Kind::File
        },
    })
}

// Pure prefix policy: it can reject UNC/device roots without any filesystem call.
fn root_drive(path: &Path) -> io::Result<u8> {
    if !path.is_absolute()
        || path.as_os_str().encode_wide().any(|unit| unit == 0)
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(invalid("explicit native absolute root required"));
    }
    if path.as_os_str().encode_wide().count() > 32768 {
        return Err(invalid("native root exceeds path budget"));
    }
    match path.components().next() {
        Some(std::path::Component::Prefix(prefix)) => match prefix.kind() {
            std::path::Prefix::Disk(drive) | std::path::Prefix::VerbatimDisk(drive) => Ok(drive),
            _ => Err(unsupported_root(
                "UNC and device namespace roots are not eligible",
            )),
        },
        _ => Err(invalid("explicit drive root required")),
    }
}

/// Open a fixed-drive root component by component, rejecting ancestor reparse points.
pub(crate) fn open_root(path: &Path) -> io::Result<File> {
    let drive = root_drive(path)?;
    let drive_root = [u16::from(drive), 58, 92, 0];
    // SAFETY: fixed NUL-terminated drive root. Reject mapped network drives before
    // opening a caller's path; later handle checks still verify local NTFS.
    if unsafe { GetDriveTypeW(drive_root.as_ptr()) } != DRIVE_FIXED {
        return Err(unsupported_root(
            "only fixed local drive roots are eligible",
        ));
    }
    let drive_path = format!("{}:\\", char::from(drive));
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(drive_path)?;
    if stamp(&file)?.kind != Kind::Directory {
        return Err(invalid("directory root required"));
    }
    for part in path.components().skip(1) {
        match part {
            std::path::Component::Normal(name) => {
                file = child(&file, name, Kind::Directory, false)?;
            }
            std::path::Component::RootDir | std::path::Component::CurDir => (),
            _ => return Err(invalid("unexpected native root component")),
        }
    }
    Ok(file)
}

/// Single-component lookup/create relative to a retained parent; no path fallback.
pub(crate) fn child(parent: &File, name: &OsStr, kind: Kind, create: bool) -> io::Result<File> {
    if stamp(parent)?.kind != Kind::Directory {
        return Err(invalid("directory parent required"));
    }
    let mut wide = component(name)?;
    let byte_length = u16::try_from(wide.len() * 2).map_err(|_| invalid("component too long"))?;
    let native_name = UNICODE_STRING {
        Length: byte_length,
        MaximumLength: byte_length,
        Buffer: wide.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &native_name,
        Attributes: OBJ_CASE_INSENSITIVE,
        ..Default::default()
    };
    let mut handle = ptr::null_mut();
    let mut status = Default::default();
    let access = FILE_GENERIC_READ
        | if create {
            FILE_GENERIC_WRITE | DELETE
        } else {
            0
        };
    let options = FILE_OPEN_REPARSE_POINT
        | FILE_SYNCHRONOUS_IO_NONALERT
        | if kind == Kind::Directory {
            FILE_DIRECTORY_FILE
        } else {
            FILE_NON_DIRECTORY_FILE
        };
    // SAFETY: SDK structs/pointers are live for this synchronous call. The name
    // contains exactly one validated component. No ACL/EA/payload is supplied.
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &attributes,
            &mut status,
            ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            if create { FILE_CREATE } else { FILE_OPEN },
            options,
            ptr::null(),
            0,
        )
    };
    if result < 0 {
        // SAFETY: pure conversion of the NTSTATUS returned above.
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(result) } as i32,
        ));
    }
    if handle.is_null() || handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
        return Err(io::Error::other("native open returned no handle"));
    }
    // SAFETY: successful NtCreateFile transferred one owned handle, closed by File.
    let file = unsafe { File::from_raw_handle(handle) };
    if stamp(&file)?.kind != kind {
        return Err(unsupported("opened type differs"));
    }
    Ok(file)
}

/// Move the held object to one child name with replacement disabled in the syscall.
#[cfg(test)]
pub(crate) fn rename_exclusive(file: &File, parent: &File, name: &OsStr) -> io::Result<()> {
    let wide = component(name)?;
    if stamp(parent)?.kind != Kind::Directory {
        return Err(invalid("directory parent required"));
    }
    let source = stamp(file)?;
    if source.volume != stamp(parent)?.volume {
        return Err(unsupported("cross-volume commit"));
    }
    let bytes = (offset_of!(FILE_RENAME_INFORMATION, FileName) + wide.len() * 2)
        .max(size_of::<FILE_RENAME_INFORMATION>());
    // u64 storage supplies SDK alignment on the validated 64-bit Windows targets.
    let mut storage = vec![0u64; bytes.div_ceil(size_of::<u64>())];
    // SAFETY: zeroed aligned storage is large enough for the SDK header + name.
    // ReplaceIfExists remains FALSE. The parent and source handles stay live.
    unsafe {
        let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        (*info).RootDirectory = parent.as_raw_handle();
        (*info).FileNameLength = (wide.len() * 2) as u32;
        ptr::copy_nonoverlapping(
            wide.as_ptr(),
            ptr::addr_of_mut!((*info).FileName).cast::<u16>(),
            wide.len(),
        );
        let mut completion = Default::default();
        let status = NtSetInformationFile(
            file.as_raw_handle(),
            &mut completion,
            info.cast(),
            bytes as u32,
            FileRenameInformation,
        );
        if status < 0 {
            return Err(io::Error::from_raw_os_error(
                RtlNtStatusToDosError(status) as i32
            ));
        }
    }
    Ok(())
}

/// Delete only the retained, operation-owned object; directories must be empty.
#[cfg(test)]
pub(crate) fn delete_owned(file: File) -> io::Result<()> {
    let _ = stamp(&file)?;
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: exact SDK struct/class; live owned handle. No pathname lookup.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&disposition as *const FILE_DISPOSITION_INFO).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
#[path = "filesystem_windows_tests.rs"]
mod tests;

#[cfg(test)]
use std::mem::offset_of;
#[cfg(test)]
use windows_sys::Wdk::Storage::FileSystem::{
    FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
};
#[path = "filesystem_windows_read.rs"]
pub(crate) mod reader;

#[path = "filesystem_windows_security.rs"]
pub(crate) mod security;

fn snapshot(native: Stamp) -> crate::validation::EntryStamp {
    let mode = if native.kind == Kind::Directory {
        0o040555
    } else {
        0o100444
    } | if native.attributes & FILE_ATTRIBUTE_READONLY == 0 {
        0o200
    } else {
        0
    };
    crate::validation::EntryStamp {
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
pub(crate) fn entry_stamp(file: &File) -> io::Result<crate::validation::EntryStamp> {
    reader::unnamed_stream_only(file)?;
    stamp(file).map(snapshot)
}
