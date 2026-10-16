use std::{fs::File, io};

// Reject ACL-bearing creation parents and newly created objects. Mode bits do
// not establish private staging when an inherited ACL can grant more access.
#[cfg(target_os = "linux")]
pub(super) fn no_acl(file: &File) -> io::Result<bool> {
    for name in ["system.posix_acl_access", "system.posix_acl_default"] {
        let mut bytes = [0u8; 256];
        match rustix::fs::fgetxattr(file, name, &mut bytes[..]) {
            Ok(0) => (),
            Ok(_) | Err(rustix::io::Errno::RANGE) => return Ok(false),
            Err(rustix::io::Errno::NODATA | rustix::io::Errno::OPNOTSUPP) => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(true)
}

#[cfg(target_os = "macos")]
pub(super) fn no_acl(file: &File) -> io::Result<bool> {
    use std::{
        ffi::{c_int, c_void},
        os::fd::AsRawFd,
    };
    // Exact SDK <sys/acl.h> declarations; ACL_TYPE_EXTENDED = 0x100.
    // Apple acl_get_fd_np retrieves FILESEC_ACL through the live descriptor;
    // an absent ACL property returns NULL/ENOENT. Reject even an explicit
    // empty ACL rather than interpreting ACL entries or inherited grants.
    unsafe extern "C" {
        fn acl_get_fd_np(fd: c_int, kind: c_int) -> *mut c_void;
        fn acl_free(object: *mut c_void) -> c_int;
    }
    // SAFETY: borrowed live File descriptor, documented extended ACL selector.
    let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), 0x100) };
    if acl.is_null() {
        let observed = io::Error::last_os_error();
        return if observed.raw_os_error() == Some(libc::ENOENT) {
            Ok(true)
        } else {
            Err(observed)
        };
    }
    // SAFETY: release exactly once the owning pointer allocated by libc.
    // Opaque ACL memory is never dereferenced or shared with another call.
    if unsafe { acl_free(acl) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(false)
}
