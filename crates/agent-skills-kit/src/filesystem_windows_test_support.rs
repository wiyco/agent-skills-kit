//! Original owned-fixture ACL helpers shared by native Windows tests only.
use super::security;
use std::{fs, os::windows::fs::OpenOptionsExt, path::Path};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE,
};

pub(crate) fn set_fixture_dacl(path: &Path, sddl: &str) {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
        },
        Storage::FileSystem::{READ_CONTROL, WRITE_DAC},
    };
    let file = fs::OpenOptions::new()
        .access_mode(READ_CONTROL | WRITE_DAC)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .unwrap();
    let text = sddl.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut pointer = std::ptr::null_mut();
    // SAFETY: trusted original fixture SDDL, terminated input and one API allocation.
    assert_ne!(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut pointer,
                std::ptr::null_mut(),
            )
        },
        0
    );
    assert!(!pointer.is_null());
    struct Descriptor(*mut core::ffi::c_void);
    impl Drop for Descriptor {
        fn drop(&mut self) {
            // SAFETY: unique allocation from successful SDDL conversion.
            unsafe {
                LocalFree(self.0);
            }
        }
    }
    let descriptor = Descriptor(pointer);
    use std::os::windows::io::AsRawHandle;
    // SAFETY: exact held owned fixture object with WRITE_DAC and live descriptor.
    let status = unsafe {
        windows_sys::Wdk::Storage::FileSystem::NtSetSecurityObject(
            file.as_raw_handle(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor.0,
        )
    };
    assert_eq!(status, 0, "set owned fixture DACL: {status:#x}");
}
pub(crate) fn fixture_dacl(path: &Path) -> String {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertSecurityDescriptorToStringSecurityDescriptorW, SDDL_REVISION_1,
            },
            DACL_SECURITY_INFORMATION,
        },
    };
    let file = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .unwrap();
    let bytes = security::descriptor(&file).unwrap();
    let mut aligned = vec![0u64; bytes.len().div_ceil(8)];
    // SAFETY: aligned initialized allocation has sufficient byte capacity.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), aligned.as_mut_ptr().cast(), bytes.len());
    }
    let mut text = std::ptr::null_mut();
    let mut length = 0;
    // SAFETY: valid bounded SDK descriptor in aligned storage and API-allocated output.
    assert_ne!(
        unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                aligned.as_mut_ptr().cast(),
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut text,
                &mut length,
            )
        },
        0
    );
    assert!(!text.is_null());
    struct Text(*mut u16);
    impl Drop for Text {
        fn drop(&mut self) {
            // SAFETY: unique SDK conversion allocation.
            unsafe {
                LocalFree(self.0.cast());
            }
        }
    }
    let text = Text(text);
    assert!((1..=65536).contains(&length));
    // SAFETY: reported allocation bound; stop at the first returned NUL.
    let units = unsafe { std::slice::from_raw_parts(text.0, length as usize) };
    let end = units.iter().position(|unit| *unit == 0).unwrap();
    String::from_utf16(&units[..end]).unwrap()
}
