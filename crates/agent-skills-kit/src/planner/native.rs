//! Platform I/O for the shared planner; policy remains in build.rs.
use std::{ffi::OsString, io, path::Path, sync::Arc};
pub(crate) type Identity = (u64, [u8; 16]);
pub(crate) type Names = Box<dyn Iterator<Item = io::Result<OsString>>>;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) type Directory = std::fs::File;
#[cfg(windows)]
pub(crate) type Directory = crate::filesystem_windows::reader::Handle;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn identity(file: &Directory) -> io::Result<Identity> {
    file.metadata()
        .map(|metadata| crate::filesystem::stamp(&metadata).file_identity())
}
#[cfg(windows)]
pub(crate) fn identity(file: &Directory) -> io::Result<Identity> {
    crate::filesystem_windows::stamp(file).map(|stamp| (stamp.volume, stamp.identity))
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn names(directory: &Directory, _max: usize) -> io::Result<Names> {
    use std::os::unix::ffi::OsStrExt;
    let reader = rustix::fs::Dir::read_from(directory)?;
    Ok(Box::new(reader.filter_map(|entry| match entry {
        Ok(entry) => {
            let bytes = entry.file_name().to_bytes();
            if matches!(bytes, b"." | b"..") {
                None
            } else {
                Some(Ok(std::ffi::OsStr::from_bytes(bytes).to_owned()))
            }
        }
        Err(error) => Some(Err(error.into())),
    })))
}
#[cfg(windows)]
pub(crate) fn names(directory: &Directory, max: usize) -> io::Result<Names> {
    Ok(Box::new(
        directory
            .entries(max)?
            .into_iter()
            .map(|entry| Ok(entry.name)),
    ))
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn parent(directory: &Arc<Directory>) -> io::Result<Arc<Directory>> {
    use rustix::fs::{Mode, OFlags};
    Ok(Arc::new(std::fs::File::from(rustix::fs::openat(
        directory,
        "..",
        crate::filesystem::flags() | OFlags::DIRECTORY,
        Mode::empty(),
    )?)))
}
#[cfg(windows)]
pub(crate) fn parent(directory: &Arc<Directory>) -> io::Result<Arc<Directory>> {
    use std::{
        ffi::OsString,
        os::windows::{ffi::OsStringExt, io::AsRawHandle},
        path::{Component, PathBuf},
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
    };
    let original = identity(directory)?;
    let mut name = vec![0u16; 32769];
    // SAFETY: live retained directory handle; fixed initialized writable UTF-16
    // buffer. The returned count excludes NUL and is checked before conversion.
    let count = unsafe {
        GetFinalPathNameByHandleW(
            directory.as_raw_handle(),
            name.as_mut_ptr(),
            name.len() as u32,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    } as usize;
    if count == 0 {
        return Err(io::Error::last_os_error());
    }
    if count >= name.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "native ancestry path exceeds its budget",
        ));
    }
    let path = PathBuf::from(OsString::from_wide(&name[..count]));
    // This is a trusted-ancestor lookup only, never a payload descendant open.
    // Caller-controlled ancestors/mount topology remain contract prerequisites.
    let verified = crate::filesystem_windows::open_root(&path)?;
    let verified = crate::filesystem_windows::stamp(&verified)?;
    if (verified.volume, verified.identity) != original {
        return Err(io::Error::other("native ancestry binding changed"));
    }
    if !path
        .components()
        .any(|part| matches!(part, Component::Normal(_)))
    {
        return Ok(Arc::clone(directory));
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("native ancestry has no parent"))?;
    Ok(Arc::new(Directory::new(
        crate::filesystem_windows::open_root(parent)?,
    )))
}
pub(crate) fn path_bytes(path: &Path) -> usize {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().count() * 2
    }
    #[cfg(not(windows))]
    {
        path.as_os_str().as_encoded_bytes().len()
    }
}
