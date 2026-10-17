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
    Win32::{Foundation::STATUS_NO_MORE_FILES, System::IO::IO_STATUS_BLOCK},
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

/// Every clone shares this explicit per-object cursor lock; there is no global lock.
#[derive(Debug)]
pub(crate) struct Handle {
    file: File,
    scan: std::sync::Mutex<()>,
}
impl Handle {
    pub(crate) fn new(file: File) -> Self {
        Self {
            file,
            scan: std::sync::Mutex::new(()),
        }
    }
    pub(crate) fn into_file(self) -> File {
        self.file
    }
    pub(crate) fn entries(&self, max: usize) -> io::Result<Vec<DirectoryEntry>> {
        let _guard = self
            .scan
            .lock()
            .map_err(|_| io::Error::other("directory scan lock poisoned"))?;
        entries_locked(&self.file, max)
    }
}
impl std::ops::Deref for Handle {
    type Target = File;
    fn deref(&self) -> &File {
        &self.file
    }
}
impl std::ops::DerefMut for Handle {
    fn deref_mut(&mut self) -> &mut File {
        &mut self.file
    }
}

fn entries_locked(directory: &File, max: usize) -> io::Result<Vec<DirectoryEntry>> {
    if max > 65536 {
        return Err(invalid("entry ceiling exceeded"));
    }
    let expected = stamp(directory)?;
    if expected.kind != Kind::Directory {
        return Err(invalid("directory scan required"));
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
                directory.as_raw_handle(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::{
            Arc, Barrier,
            atomic::{AtomicU64, Ordering},
        },
        time::Duration,
    };
    #[test]
    fn repeated_and_concurrent_scans_share_one_explicit_cursor_guard() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        struct Root(std::path::PathBuf);
        impl Drop for Root {
            fn drop(&mut self) {
                fs::remove_dir_all(&self.0).expect("owned cursor fixture teardown");
            }
        }
        let root = (0..100)
            .find_map(|_| {
                let path = std::env::temp_dir().join(format!(
                    "ask-windows-cursor-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&path) {
                    Ok(()) => Some(Root(path)),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => None,
                    Err(error) => panic!("owned cursor root: {error}"),
                }
            })
            .expect("cursor fixture collision budget");
        for name in ["a", "b"] {
            fs::write(root.0.join(name), b"inert").unwrap();
        }
        let held = Arc::new(Handle::new(open_root(&root.0).unwrap()));
        let names = |entries: Vec<DirectoryEntry>| {
            entries
                .into_iter()
                .map(|entry| entry.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            held.entries(1).unwrap_err().kind(),
            io::ErrorKind::FileTooLarge
        );
        for _ in 0..2 {
            assert_eq!(
                names(held.entries(2).unwrap()),
                vec![OsString::from("a"), OsString::from("b")]
            );
        }
        let barrier = Arc::new(Barrier::new(2));
        let (sender, receiver) = std::sync::mpsc::channel();
        let threads = (0..2)
            .map(|_| {
                let held = Arc::clone(&held);
                let barrier = Arc::clone(&barrier);
                let sender = sender.clone();
                std::thread::spawn(move || {
                    for _ in 0..16 {
                        barrier.wait();
                        let result = held.entries(2).map(|entries| {
                            entries
                                .into_iter()
                                .map(|entry| entry.name)
                                .collect::<Vec<_>>()
                        });
                        sender.send(result).unwrap();
                    }
                })
            })
            .collect::<Vec<_>>();
        let observations = (0..32)
            .map(|_| {
                receiver
                    .recv_timeout(Duration::from_secs(10))
                    .expect("bounded concurrent scan")
            })
            .collect::<Vec<_>>();
        for thread in threads {
            thread.join().unwrap();
        }
        for observation in observations {
            assert_eq!(
                observation.unwrap(),
                vec![OsString::from("a"), OsString::from("b")]
            );
        }
    }
}
