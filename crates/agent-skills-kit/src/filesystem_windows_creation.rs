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
    /// Payloads start private but use the token's default destination owner/group.
    pub(crate) fn private_payload(token: &Token) -> io::Result<Self> {
        Self::from_sddl(&format!("D:P(A;OICI;FA;;;{})", token.user_sid()?))
    }
    pub(crate) fn private_file(token: &Token) -> io::Result<Self> {
        Self::from_sddl(&format!("D:P(A;;FA;;;{})", token.user_sid()?))
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
        ACCESS_ALLOWED_ACE, Authorization::ConvertSecurityDescriptorToStringSecurityDescriptorW,
        CONTAINER_INHERIT_ACE, EqualSid, GetAce, GetSecurityDescriptorControl,
        GetSecurityDescriptorDacl, GetSecurityDescriptorOwner, INHERITED_ACE, OBJECT_INHERIT_ACE,
        SE_DACL_PROTECTED,
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
        let units = unsafe { std::slice::from_raw_parts(text, length as usize) };
        let end = units
            .iter()
            .position(|unit| *unit == 0)
            .expect("terminated SDK string");
        let text = String::from_utf16(&units[..end]).unwrap();
        drop(owned);
        (text, control & SE_DACL_PROTECTED != 0)
    }
    fn assert_user_only(file: &File, sid: &str, protected: bool, directory: bool) {
        let bytes = super::super::descriptor(file).unwrap();
        let mut aligned = vec![0u64; bytes.len().div_ceil(8)];
        // SAFETY: aligned storage has sufficient initialized capacity for this SDK descriptor.
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), aligned.as_mut_ptr().cast(), bytes.len());
        }
        let descriptor = aligned.as_mut_ptr().cast();
        let expected = Descriptor::from_sddl(&format!("O:{sid}D:P")).unwrap();
        let mut owner = ptr::null_mut();
        let mut expected_owner = ptr::null_mut();
        let mut defaulted = 0;
        let mut present = 0;
        let mut acl = ptr::null_mut();
        let mut raw_ace = ptr::null_mut();
        // SAFETY: both descriptors are valid SDK-produced, aligned and live. The
        // returned owner/ACL/ACE pointers remain borrowed from those descriptors.
        unsafe {
            assert_ne!(
                GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted),
                0
            );
            assert_ne!(
                GetSecurityDescriptorOwner(expected.as_ptr(), &mut expected_owner, &mut defaulted),
                0
            );
            // The protected stage explicitly sets its owner. Inherited children
            // use the token default owner (which may be an owner-capable group).
            if protected {
                assert_ne!(EqualSid(owner, expected_owner), 0);
            }
            assert_ne!(
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted),
                0
            );
            assert_ne!(present, 0);
            assert!(!acl.is_null());
            assert_eq!((*acl).AceCount, 1);
            assert_ne!(GetAce(acl, 0, &mut raw_ace), 0);
            assert!(!raw_ace.is_null());
            let ace = raw_ace.cast::<ACCESS_ALLOWED_ACE>();
            // ACCESS_ALLOWED_ACE_TYPE is the SDK's type 0; no other ACE shape is accepted.
            assert_eq!((*ace).Header.AceType, 0);
            assert!((*ace).Header.AceSize as usize >= size_of::<ACCESS_ALLOWED_ACE>());
            assert_eq!(
                (*ace).Mask,
                windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS
            );
            let flags = if directory {
                OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE
            } else {
                INHERITED_ACE
            };
            assert_eq!(u32::from((*ace).Header.AceFlags), flags);
            assert_ne!(
                EqualSid(
                    ptr::addr_of!((*ace).SidStart).cast_mut().cast(),
                    expected_owner
                ),
                0
            );
        }
        assert_eq!(dacl(file).1, protected);
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
        assert_user_only(&stage, &sid, true, true);
        let child = native::child(&stage, OsStr::new("inert child"), Kind::File, true).unwrap();
        let inherited = dacl(&child).0;
        assert_user_only(&child, &sid, false, false);
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
    // Fixture-only comparison for simple allow ACEs. Duplicate grants and the
    // INHERITED/auto-inheritance provenance flags do not change these permissions.
    // Unknown ACE types/flags fail; this is not a general Windows ACL evaluator.
    fn allow_policy(file: &File) -> std::collections::BTreeMap<(Vec<u8>, u8), u32> {
        use windows_sys::Win32::Security::{GetLengthSid, INHERIT_ONLY_ACE};
        let bytes = super::super::descriptor(file).unwrap();
        let mut aligned = vec![0u64; bytes.len().div_ceil(8)];
        let mut acl = ptr::null_mut();
        let mut present = 0;
        let mut defaulted = 0;
        let mut result = std::collections::BTreeMap::new();
        // SAFETY: bounded SDK descriptor copied to sufficiently large aligned
        // storage. SDK-returned ACL/ACE/SID pointers remain live in that storage.
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), aligned.as_mut_ptr().cast(), bytes.len());
            assert_ne!(
                GetSecurityDescriptorDacl(
                    aligned.as_mut_ptr().cast(),
                    &mut present,
                    &mut acl,
                    &mut defaulted
                ),
                0
            );
            assert_ne!(present, 0);
            assert!(!acl.is_null());
            for index in 0..u32::from((*acl).AceCount) {
                let mut raw = ptr::null_mut();
                assert_ne!(GetAce(acl, index, &mut raw), 0);
                assert!(!raw.is_null());
                let ace = raw.cast::<ACCESS_ALLOWED_ACE>();
                assert_eq!((*ace).Header.AceType, 0);
                assert!((*ace).Header.AceSize as usize >= size_of::<ACCESS_ALLOWED_ACE>());
                let flags = u32::from((*ace).Header.AceFlags);
                assert_eq!(
                    flags
                        & !(OBJECT_INHERIT_ACE
                            | CONTAINER_INHERIT_ACE
                            | INHERITED_ACE
                            | INHERIT_ONLY_ACE),
                    0
                );
                let sid = ptr::addr_of!((*ace).SidStart).cast_mut().cast();
                let length = GetLengthSid(sid) as usize;
                assert!((8..=68).contains(&length));
                let sid = std::slice::from_raw_parts(sid.cast::<u8>(), length).to_vec();
                for (role, active) in [
                    (0, flags & INHERIT_ONLY_ACE == 0),
                    (1, flags & OBJECT_INHERIT_ACE != 0),
                    (2, flags & CONTAINER_INHERIT_ACE != 0),
                ] {
                    if active {
                        *result.entry((sid.clone(), role)).or_insert(0) |= (*ace).Mask;
                    }
                }
            }
        }
        result
    }
    #[test]
    fn committed_payload_inherits_the_real_destination_like_native_creation() {
        use std::io::Write;
        for inherit_user in [true, false] {
            let root = Root::new();
            let anchor = native::open_root(&root.0).unwrap();
            let token = Token::effective().unwrap();
            let sid = token.user_sid().unwrap();
            let user_flags = if inherit_user { "OICI" } else { "" };
            let parent_descriptor = Descriptor::from_sddl(&format!(
                "O:{sid}D:P(A;{user_flags};FA;;;{sid})(A;OICI;FR;;;WD)(A;OICI;FA;;;CO)"
            ))
            .unwrap();
            let parent = native::create_with_security(
                &anchor,
                OsStr::new("destination parent"),
                Kind::Directory,
                &parent_descriptor,
            )
            .unwrap();
            let baseline = native::child(
                &parent,
                OsStr::new("native baseline"),
                Kind::Directory,
                true,
            )
            .unwrap();
            let baseline_child =
                native::child(&baseline, OsStr::new("resource"), Kind::File, true).unwrap();
            let expected_root = allow_policy(&baseline);
            let expected_child = allow_policy(&baseline_child);
            assert!(dacl(&baseline).0.contains(";;;WD)"));
            let parent_security = super::super::descriptor(&parent).unwrap();
            let stage_descriptor = Descriptor::private_for(&token).unwrap();
            let stage = native::create_with_security(
                &parent,
                OsStr::new("stage"),
                Kind::Directory,
                &stage_descriptor,
            )
            .unwrap();
            let payload_descriptor = Descriptor::private_payload(&token).unwrap();
            let payload = native::create_with_security(
                &stage,
                OsStr::new("payload"),
                Kind::Directory,
                &payload_descriptor,
            )
            .unwrap();
            let file_descriptor = Descriptor::private_file(&token).unwrap();
            let mut child = native::create_payload_entry(
                &payload,
                OsStr::new("resource"),
                Kind::File,
                &file_descriptor,
            )
            .unwrap();
            child.write_all(b"inert original bytes").unwrap();
            assert_eq!(
                dacl(&payload),
                dacl(&stage),
                "every staged object must remain private despite traversal bypass privileges"
            );
            assert!(dacl(&child).1);
            assert!(!dacl(&child).0.contains(";;;WD)"));
            let child_private = allow_policy(&child);
            native::rename_exclusive(&payload, &parent, OsStr::new("installed")).unwrap();
            assert_eq!(
                dacl(&payload),
                dacl(&stage),
                "rename must not be assumed to establish destination inheritance"
            );
            finalize_inheritance(&payload, &parent, &token).unwrap();
            assert_eq!(
                allow_policy(&payload),
                expected_root,
                "root inheritance, inherit_user={inherit_user}"
            );
            assert!(!dacl(&payload).1);
            assert_eq!(
                allow_policy(&child),
                child_private,
                "root update must not recursively modify descendants"
            );
            finalize_inheritance(&child, &payload, &token).unwrap();
            assert_eq!(
                allow_policy(&child),
                expected_child,
                "real inherited grants must propagate through the complete payload"
            );
            assert_eq!(parent_security, super::super::descriptor(&parent).unwrap());
            assert_eq!(
                fs::read(root.0.join("destination parent/installed/resource")).unwrap(),
                b"inert original bytes"
            );
        }
    }

    #[test]
    fn permission_finalization_failure_retains_the_committed_private_payload() {
        let root = Root::new();
        let anchor = native::open_root(&root.0).unwrap();
        let token = Token::effective().unwrap();
        let private = Descriptor::private_for(&token).unwrap();
        let stage =
            native::create_with_security(&anchor, OsStr::new("stage"), Kind::Directory, &private)
                .unwrap();
        // This trusted fixture deliberately holds no WRITE_DAC capability.
        let payload = native::child(&stage, OsStr::new("payload"), Kind::Directory, true).unwrap();
        native::rename_exclusive(&payload, &anchor, OsStr::new("committed")).unwrap();
        let before = allow_policy(&payload);
        let error = finalize_inheritance(&payload, &anchor, &token).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(before, allow_policy(&payload));
        assert!(
            root.0.join("committed").is_dir(),
            "post-commit failure is never rollback authority"
        );
        assert!(!root.0.join("stage/payload").exists());
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

/// Apply real destination inheritance only after native commit. This can fail or
/// leave a partially finalized tree; callers must report a committed payload,
/// never delete it. Each inventoried object requires its own bounded update.
pub(crate) fn finalize_inheritance(file: &File, parent: &File, token: &Token) -> io::Result<()> {
    use windows_sys::Win32::{
        Security::{
            CreatePrivateObjectSecurityEx, DestroyPrivateObjectSecurity, GENERIC_MAPPING,
            GetSecurityDescriptorDacl, SEF_DACL_AUTO_INHERIT,
            UNPROTECTED_DACL_SECURITY_INFORMATION,
        },
        Storage::FileSystem::{
            FILE_ALL_ACCESS, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
        },
    };
    let bytes = super::descriptor(parent)?;
    let mut aligned = vec![0u64; bytes.len().div_ceil(8)];
    // SAFETY: aligned initialized buffer has at least bytes.len() bytes available.
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), aligned.as_mut_ptr().cast(), bytes.len());
    }
    struct Inherited(PSECURITY_DESCRIPTOR);
    impl Drop for Inherited {
        fn drop(&mut self) {
            // SAFETY: uniquely owned output of CreatePrivateObjectSecurityEx below.
            unsafe {
                DestroyPrivateObjectSecurity(&mut self.0);
            }
        }
    }
    let mapping = GENERIC_MAPPING {
        GenericRead: FILE_GENERIC_READ,
        GenericWrite: FILE_GENERIC_WRITE,
        GenericExecute: FILE_GENERIC_EXECUTE,
        GenericAll: FILE_ALL_ACCESS,
    };
    let mut result = ptr::null_mut();
    let is_directory = i32::from(
        crate::filesystem_windows::stamp(file)?.kind == crate::filesystem_windows::Kind::Directory,
    );
    // SAFETY: SDK-produced bounded parent descriptor, aligned copy, live query
    // token and exact file rights mapping. No source/creator descriptor or owner/
    // privilege bypass flags; the result has its documented distinct destructor.
    if unsafe {
        CreatePrivateObjectSecurityEx(
            aligned.as_mut_ptr().cast(),
            ptr::null_mut(),
            &mut result,
            ptr::null(),
            is_directory,
            SEF_DACL_AUTO_INHERIT,
            token.0,
            &mapping,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if result.is_null() {
        return Err(io::Error::other(
            "no destination inheritance descriptor returned",
        ));
    }
    let inherited = Inherited(result);
    // SAFETY: successful SDK descriptor; size is checked before extracting its DACL.
    if !(20..=65536).contains(&unsafe { GetSecurityDescriptorLength(inherited.0) }) {
        return Err(io::Error::other(
            "destination inheritance descriptor exceeds bounds",
        ));
    }
    let mut present = 0;
    let mut defaulted = 0;
    let mut acl = ptr::null_mut();
    // SAFETY: descriptor remains live, writable typed outputs, borrowed DACL.
    if unsafe { GetSecurityDescriptorDacl(inherited.0, &mut present, &mut acl, &mut defaulted) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    if present == 0 || acl.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "null destination DACL is not supported",
        ));
    }
    // SAFETY: live committed operation-owned handle with WRITE_DAC and bounded
    // SDK self-relative descriptor. This updates exactly the held object, without
    // a recursive user-space namespace walk or source metadata. Each inventoried
    // child must be finalized separately through its own retained handle.
    let status = unsafe {
        windows_sys::Wdk::Storage::FileSystem::NtSetSecurityObject(
            file.as_raw_handle(),
            DACL_SECURITY_INFORMATION | UNPROTECTED_DACL_SECURITY_INFORMATION,
            inherited.0,
        )
    };
    if status < 0 {
        Err(io::Error::from_raw_os_error(unsafe {
            windows_sys::Win32::Foundation::RtlNtStatusToDosError(status)
        } as i32))
    } else {
        Ok(())
    }
}
