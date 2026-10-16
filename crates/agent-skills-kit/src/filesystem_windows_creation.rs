//! Native creation descriptors; no changes to existing filesystem objects.
use super::*;
use std::mem::size_of;
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_TOKEN, HANDLE},
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SDDL_REVISION_1,
        },
        GetTokenInformation, PSECURITY_DESCRIPTOR, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    System::Threading::{GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken},
};

pub(crate) struct Token(HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: uniquely owned token returned by the successful open below.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
impl Token {
    pub(crate) fn effective() -> io::Result<Self> {
        let mut handle = ptr::null_mut();
        // SAFETY: current-thread pseudo handle, valid output; query only. OpenAsSelf
        // selects access-check context, not a different token's security identity.
        if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut handle) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_NO_TOKEN as i32) {
                return Err(error);
            }
            // SAFETY: no impersonation token exists; query the current process token.
            if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) } == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        if handle.is_null() {
            return Err(io::Error::other("token open returned no handle"));
        }
        Ok(Self(handle))
    }
    pub(crate) fn user_sid(&self) -> io::Result<String> {
        let mut length = 0;
        // SAFETY: sizing query writes only length; the token handle remains live.
        let sized =
            unsafe { GetTokenInformation(self.0, TokenUser, ptr::null_mut(), 0, &mut length) };
        if sized != 0
            || io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
            || !(size_of::<TOKEN_USER>() as u32..=65536).contains(&length)
        {
            return Err(io::Error::other("invalid token-user buffer size"));
        }
        let mut buffer = vec![0u64; (length as usize).div_ceil(8)];
        let capacity = buffer.len() * 8;
        // SAFETY: aligned initialized buffer has at least the requested byte size.
        if unsafe {
            GetTokenInformation(
                self.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                capacity as u32,
                &mut length,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if length as usize > capacity || (length as usize) < size_of::<TOKEN_USER>() {
            return Err(io::Error::other("invalid token-user completion size"));
        }
        // SAFETY: successful API returned the exact TOKEN_USER layout in aligned storage.
        let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
        let mut text = ptr::null_mut();
        // SAFETY: SID borrows the live API-filled token buffer; output is API allocated.
        if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if text.is_null() {
            return Err(io::Error::other("SID conversion returned no string"));
        }
        let owned = Descriptor {
            pointer: text.cast(),
        };
        // Windows SIDs have at most 15 subauthorities; 256 UTF-16 units bounds their
        // canonical numeric text. The allocation is a valid NUL-terminated API string.
        let mut length = 0;
        while length < 256 && unsafe { *text.add(length) } != 0 {
            length += 1;
        }
        if length == 256 {
            return Err(io::Error::other("SID text exceeds bounds"));
        }
        // SAFETY: only the initialized units preceding the API string terminator.
        let result = String::from_utf16(unsafe { std::slice::from_raw_parts(text, length) })
            .map_err(|_| io::Error::other("SID text is not valid Unicode"));
        drop(owned);
        result
    }
}

pub(crate) struct Descriptor {
    pointer: PSECURITY_DESCRIPTOR,
}
impl Descriptor {
    pub(crate) fn as_ptr(&self) -> PSECURITY_DESCRIPTOR {
        self.pointer
    }
    fn from_sddl(text: &str) -> io::Result<Self> {
        let text = text.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let mut pointer = ptr::null_mut();
        // SAFETY: internally constructed, terminated SDDL, current revision and one
        // allocated output. No untrusted skill or environment text enters this call.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut pointer,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if pointer.is_null() {
            return Err(io::Error::other("no creation descriptor returned"));
        }
        Ok(Self { pointer })
    }
    pub(crate) fn private_for(token: &Token) -> io::Result<Self> {
        let sid = token.user_sid()?;
        // Protected DACL: exactly the effective user, full file rights, inheritable
        // by files/directories. No inherited broad grant or null DACL interval.
        Self::from_sddl(&format!("O:{sid}D:P(A;OICI;FA;;;{sid})"))
    }
}
impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: allocation/destructor pairing is fixed by the private constructors;
        // the pointer is uniquely owned and no longer borrowed by a native call.
        unsafe {
            LocalFree(self.pointer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filesystem_windows::{self as native, Kind};
    use std::{
        ffi::OsStr,
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };
    use windows_sys::Win32::Security::{
        Authorization::ConvertSecurityDescriptorToStringSecurityDescriptorW,
        GetSecurityDescriptorControl, SE_DACL_PROTECTED,
    };
    struct Root(PathBuf);
    impl Root {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            for _ in 0..100 {
                let path = std::env::temp_dir().join(format!(
                    "ask-creation-security-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
                    Err(error) => panic!("exclusive security fixture: {error}"),
                }
            }
            panic!("exclusive fixture collision budget");
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("owned security fixture teardown");
        }
    }
    fn dacl(file: &File) -> (String, bool) {
        let bytes = super::super::descriptor(file).unwrap();
        let mut aligned = vec![0u64; bytes.len().div_ceil(8)];
        // SAFETY: initialized aligned storage is at least bytes.len() bytes long.
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), aligned.as_mut_ptr().cast(), bytes.len());
        }
        let pointer = aligned.as_mut_ptr().cast();
        let mut control = 0;
        let mut revision = 0;
        let mut text = ptr::null_mut();
        let mut length = 0;
        // SAFETY: valid bounded self-relative descriptor copied into aligned storage;
        // typed writable outputs. String is freed by the allocation guard below.
        unsafe {
            assert_ne!(
                GetSecurityDescriptorControl(pointer, &mut control, &mut revision),
                0
            );
            assert_ne!(
                ConvertSecurityDescriptorToStringSecurityDescriptorW(
                    pointer,
                    SDDL_REVISION_1,
                    DACL_SECURITY_INFORMATION,
                    &mut text,
                    &mut length
                ),
                0
            );
        }
        assert!(!text.is_null());
        let owned = Descriptor {
            pointer: text.cast(),
        };
        assert!((1..=65536).contains(&length));
        // SAFETY: API-reported length includes the NUL; conversion excludes it.
        let text =
            String::from_utf16(unsafe { std::slice::from_raw_parts(text, length as usize - 1) })
                .unwrap();
        drop(owned);
        (text, control & SE_DACL_PROTECTED != 0)
    }
    #[test]
    fn private_creation_blocks_inherited_broad_grants_in_the_create_syscall() {
        let root = Root::new();
        let anchor = native::open_root(&root.0).unwrap();
        let token = Token::effective().unwrap();
        let sid = token.user_sid().unwrap();
        let broad =
            Descriptor::from_sddl(&format!("O:{sid}D:P(A;OICI;FA;;;{sid})(A;OICI;FR;;;WD)"))
                .unwrap();
        let parent = native::create_with_security(
            &anchor,
            OsStr::new("broad parent"),
            Kind::Directory,
            &broad,
        )
        .unwrap();
        assert!(
            dacl(&parent).0.contains(";;;WD)"),
            "fixture must establish a real inheritable broad grant"
        );
        let before = super::super::descriptor(&parent).unwrap();
        let private = Descriptor::private_for(&token).unwrap();
        let stage = native::create_with_security(
            &parent,
            OsStr::new("private stage 中"),
            Kind::Directory,
            &private,
        )
        .unwrap();
        assert_eq!(dacl(&stage), (format!("D:P(A;OICI;FA;;;{sid})"), true));
        let child = native::child(&stage, OsStr::new("inert child"), Kind::File, true).unwrap();
        let inherited = dacl(&child).0;
        assert!(inherited.contains(&sid));
        assert!(!inherited.contains(";;;WD)"));
        assert_eq!(
            before,
            super::super::descriptor(&parent).unwrap(),
            "existing parent DACL must remain untouched"
        );
        assert_eq!(
            native::stamp(&stage).unwrap().volume,
            native::stamp(&parent).unwrap().volume
        );
    }
    #[test]
    fn private_create_collision_preserves_existing_bytes_and_security() {
        let root = Root::new();
        fs::write(root.0.join("winner"), b"existing winner").unwrap();
        let anchor = native::open_root(&root.0).unwrap();
        let winner = native::child(&anchor, OsStr::new("winner"), Kind::File, false).unwrap();
        let before = super::super::descriptor(&winner).unwrap();
        let token = Token::effective().unwrap();
        let private = Descriptor::private_for(&token).unwrap();
        assert_eq!(
            native::create_with_security(&anchor, OsStr::new("winner"), Kind::File, &private)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(root.0.join("winner")).unwrap(), b"existing winner");
        assert_eq!(before, super::super::descriptor(&winner).unwrap());
    }
}
