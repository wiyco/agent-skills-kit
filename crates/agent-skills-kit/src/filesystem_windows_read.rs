//! Bounded native enumeration and local-volume eligibility for read-only scans.
use super::*;
use std::mem::size_of_val;
use std::{ffi::OsString, os::windows::ffi::OsStringExt};
use windows_sys::{
    Wdk::{
        Storage::FileSystem::{
            FILE_ID_BOTH_DIR_INFORMATION, FileFsDeviceInformation, FileIdBothDirectoryInformation,
            NtQueryDirectoryFile, NtQueryVolumeInformationFile,
        },
        System::SystemServices::{
            FILE_FS_DEVICE_INFORMATION, FILE_REMOTE_DEVICE, FILE_REMOTE_DEVICE_VSMB,
        },
    },
    Win32::{
        Foundation::{INVALID_HANDLE_VALUE, STATUS_NO_MORE_FILES},
        System::IO::IO_STATUS_BLOCK,
    },
};

pub(crate) fn local_ntfs(file: &File) -> io::Result<()> {
    let mut filesystem = [0u16; 32];
    // SAFETY: live file; fixed writable filesystem-name buffer; unused outputs null.
    if unsafe {
        GetVolumeInformationByHandleW(
            file.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            filesystem.as_mut_ptr(),
            filesystem.len() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let end = filesystem
        .iter()
        .position(|unit| *unit == 0)
        .ok_or_else(|| unsupported("unbounded filesystem label"))?;
    if filesystem[..end] != [78, 84, 70, 83] {
        return Err(unsupported("only reviewed local NTFS volumes are eligible"));
    }
    let mut device = FILE_FS_DEVICE_INFORMATION::default();
    let mut completion = IO_STATUS_BLOCK::default();
    // SAFETY: exact SDK device-information layout, synchronous call, live handle.
    let status = unsafe {
        NtQueryVolumeInformationFile(
            file.as_raw_handle(),
            &mut completion,
            (&mut device as *mut FILE_FS_DEVICE_INFORMATION).cast(),
            size_of::<FILE_FS_DEVICE_INFORMATION>() as u32,
            FileFsDeviceInformation,
        )
    };
    if status < 0 {
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32,
        ));
    }
    if completion.Information != size_of::<FILE_FS_DEVICE_INFORMATION>()
        || device.Characteristics & (FILE_REMOTE_DEVICE | FILE_REMOTE_DEVICE_VSMB) != 0
    {
        return Err(unsupported(
            "remote or unrecognized volume device information",
        ));
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) struct DirectoryEntry {
    pub(crate) name: OsString,
    pub(crate) attributes: u32,
}

/// A separate native open owns this scan's cursor; cloned retained handles do not.
pub(crate) fn entries(directory: &File, max: usize) -> io::Result<Vec<DirectoryEntry>> {
    if max > 65536 {
        return Err(invalid("entry ceiling exceeded"));
    }
    let expected = stamp(directory)?;
    if expected.kind != Kind::Directory {
        return Err(invalid("directory scan required"));
    }
    // SAFETY: live validated disk-directory handle, read-only access, independent
    // open/cursor. Delete sharing accommodates an original operation-owned handle;
    // that original still denies every other opener delete access.
    let handle = unsafe {
        ReOpenFile(
            directory.as_raw_handle(),
            FILE_GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: ReOpenFile returned one new owned handle.
    let reader = unsafe { File::from_raw_handle(handle) };
    if stamp(&reader)? != expected {
        return Err(io::Error::other("directory changed while opening scan"));
    }
    let mut result = Vec::new();
    let mut restart = true;
    // One fixed-size record is enough for a maximum NTFS component. Reading one
    // record per syscall bounds allocations and avoids trusting next-offset chains.
    for _ in 0..max.saturating_add(3) {
        let mut buffer = [0u64; 128];
        let mut completion = IO_STATUS_BLOCK::default();
        // SAFETY: synchronous handle, aligned writable buffer, exact class,
        // no callbacks/event/filter. The completion byte count is checked below.
        let status = unsafe {
            NtQueryDirectoryFile(
                reader.as_raw_handle(),
                ptr::null_mut(),
                None,
                ptr::null(),
                &mut completion,
                buffer.as_mut_ptr().cast(),
                size_of_val(&buffer) as u32,
                FileIdBothDirectoryInformation,
                true,
                ptr::null(),
                restart,
            )
        };
        restart = false;
        if status == STATUS_NO_MORE_FILES {
            if stamp(directory)? != expected {
                return Err(io::Error::other("directory changed during scan"));
            }
            result.sort_by(|a: &DirectoryEntry, b| a.name.cmp(&b.name));
            if result.windows(2).any(|pair| pair[0].name == pair[1].name) {
                return Err(io::Error::other("duplicate directory observation"));
            }
            return Ok(result);
        }
        if status < 0 {
            return Err(io::Error::from_raw_os_error(
                unsafe { RtlNtStatusToDosError(status) } as i32,
            ));
        }
        let used = completion.Information;
        let offset = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFORMATION, FileName);
        if used < offset + 2 || used > size_of_val(&buffer) {
            return Err(io::Error::other("incomplete native directory record"));
        }
        // SAFETY: fixed aligned buffer has at least the SDK struct size; fields
        // precede the checked name offset. No pointer field is interpreted.
        let record = unsafe { &*buffer.as_ptr().cast::<FILE_ID_BOTH_DIR_INFORMATION>() };
        let length = record.FileNameLength as usize;
        if record.NextEntryOffset != 0
            || length == 0
            || length % 2 != 0
            || length > 510
            || offset + length > used
        {
            return Err(io::Error::other("invalid native directory record bounds"));
        }
        // SAFETY: byte range validated above, u16 aligned within the SDK layout.
        let wide = unsafe {
            std::slice::from_raw_parts(
                buffer.as_ptr().cast::<u8>().add(offset).cast::<u16>(),
                length / 2,
            )
        };
        if wide == [46] || wide == [46, 46] {
            continue;
        }
        if result.len() == max {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "entry count limit",
            ));
        }
        result.push(DirectoryEntry {
            name: OsString::from_wide(wide),
            attributes: record.FileAttributes,
        });
    }
    Err(io::Error::new(
        io::ErrorKind::FileTooLarge,
        "directory enumeration did not terminate within its budget",
    ))
}

/// Reject alternate data streams rather than silently omitting hidden payload.
pub(crate) fn unnamed_stream_only(file: &File) -> io::Result<()> {
    let mut storage = [0u64; 128];
    // SAFETY: exact stream-information class, aligned writable fixed-size buffer,
    // live disk-object handle. A larger result fails rather than reallocating.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileStreamInfo,
            storage.as_mut_ptr().cast(),
            size_of_val(&storage) as u32,
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(windows_sys::Win32::Foundation::ERROR_HANDLE_EOF as i32) {
            return Ok(());
        }
        return Err(error);
    }
    // Local NTFS has at most one unnamed data stream. Any second record is
    // already unsupported, so no attacker-controlled offset chain is traversed.
    // SAFETY: initialized aligned storage is larger than the exact SDK header.
    let record = unsafe { &*storage.as_ptr().cast::<FILE_STREAM_INFO>() };
    let name_offset = std::mem::offset_of!(FILE_STREAM_INFO, StreamName);
    if record.NextEntryOffset != 0 || record.StreamNameLength != 14 {
        return Err(unsupported("alternate stream rejected"));
    }
    // SAFETY: the seven-unit name lies inside the fixed initialized buffer and
    // the SDK name offset is u16 aligned. No offset comes from file contents.
    let name = unsafe {
        std::slice::from_raw_parts(
            storage.as_ptr().cast::<u8>().add(name_offset).cast::<u16>(),
            7,
        )
    };
    if name != [58, 58, 36, 68, 65, 84, 65] {
        return Err(unsupported("alternate stream rejected"));
    }
    Ok(())
}
