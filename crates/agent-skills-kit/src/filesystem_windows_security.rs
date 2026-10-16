//! Read-only destination security preconditions; no source ACL copying.
use std::{fs::File, io, os::windows::io::AsRawHandle, ptr};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
        DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, GetSecurityDescriptorLength,
        OWNER_SECURITY_INFORMATION,
    },
};

/// Capture only owner/group/DACL, without requesting privileged SACL access.
pub(crate) fn descriptor(file: &File) -> io::Result<Vec<u8>> {
    let mut allocated = ptr::null_mut();
    // SAFETY: live retained handle; the API allocates one descriptor, all optional
    // interior pointers are null, and the matching LocalFree guard owns it below.
    let result = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut allocated,
        )
    };
    if result != 0 {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    if allocated.is_null() {
        return Err(io::Error::other("no security descriptor returned"));
    }
    struct Allocated(*mut core::ffi::c_void);
    impl Drop for Allocated {
        fn drop(&mut self) {
            // SAFETY: exactly the allocation returned by successful GetSecurityInfo.
            unsafe {
                LocalFree(self.0);
            }
        }
    }
    let owned = Allocated(allocated);
    // SAFETY: successful GetSecurityInfo returned a valid self-relative descriptor.
    let size = unsafe { GetSecurityDescriptorLength(owned.0) } as usize;
    if !(20..=65536).contains(&size) {
        return Err(io::Error::other(
            "security descriptor exceeds inspection bounds",
        ));
    }
    // SAFETY: returned self-relative descriptor occupies its API-reported length;
    // copy into owned bounded memory before LocalFree runs.
    Ok(unsafe { std::slice::from_raw_parts(owned.0.cast::<u8>(), size) }.to_vec())
}

#[path = "filesystem_windows_creation.rs"]
pub(crate) mod creation;
