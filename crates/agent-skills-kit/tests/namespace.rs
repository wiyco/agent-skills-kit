use agent_skills_kit::planner::*;
#[cfg(not(windows))]
use std::path::Path;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "ask-namespace-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("exclusive fixture root: {error}"),
            }
        }
        panic!("fixture collision budget exhausted")
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove owned fixture root");
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
#[test]
fn namespace_observation_is_explicitly_unsupported_without_native_backend() {
    let root = Root::new();
    assert_eq!(
        observe_destination(&root.0, Path::new("skills/example"))
            .unwrap_err()
            .kind,
        NamespaceErrorKind::UnsupportedPlatform
    );
    assert_eq!(
        observe_destination(&root.0.join("absent"), Path::new("example"))
            .unwrap_err()
            .kind,
        NamespaceErrorKind::UnsupportedPlatform
    );
    assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod native {
    use super::*;
    use std::os::unix::{fs::PermissionsExt, fs::symlink};

    fn tree(root: &Path) -> Vec<(PathBuf, Vec<u8>, u32)> {
        use std::os::unix::fs::MetadataExt;
        fn walk(root: &Path, relative: &Path, out: &mut Vec<(PathBuf, Vec<u8>, u32)>) {
            let path = root.join(relative);
            let metadata = fs::symlink_metadata(&path).unwrap();
            out.push((
                relative.to_owned(),
                if metadata.is_file() {
                    fs::read(&path).unwrap()
                } else {
                    Vec::new()
                },
                metadata.mode(),
            ));
            if metadata.is_dir() {
                for entry in fs::read_dir(path).unwrap() {
                    walk(root, &relative.join(entry.unwrap().file_name()), out);
                }
            }
        }
        let mut result = Vec::new();
        walk(root, Path::new(""), &mut result);
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    #[test]
    fn missing_parents_are_observed_without_creation_or_hidden_state() {
        let root = Root::new();
        let snapshot = observe_destination(&root.0, Path::new(".agents/skills/example")).unwrap();
        assert_eq!(snapshot.presence(), DestinationPresence::Missing);
        assert_eq!(snapshot.trusted_anchor(), root.0);
        assert_eq!(snapshot.existing_directories().count(), 1);
        assert_eq!(
            snapshot.missing_components(),
            [".agents", "skills", "example"].map(std::ffi::OsString::from)
        );
        assert_eq!(
            snapshot.destination(),
            fs::canonicalize(&root.0)
                .unwrap()
                .join(".agents/skills/example")
        );
        assert!(snapshot.is_current().unwrap());
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
        fs::create_dir(root.0.join(".agents")).unwrap();
        assert!(!snapshot.is_current().unwrap());
    }

    #[test]
    fn absent_trusted_root_records_the_actual_existing_anchor() {
        let root = Root::new();
        let requested_root = root.0.join("new home/config");
        let snapshot = observe_destination(&requested_root, Path::new("skills/example")).unwrap();
        assert_eq!(
            snapshot.requested_path(),
            requested_root.join("skills/example")
        );
        assert_eq!(snapshot.trusted_anchor(), root.0);
        assert_eq!(
            snapshot.missing_components(),
            ["new home", "config", "skills", "example"].map(std::ffi::OsString::from)
        );
        assert!(snapshot.is_current().unwrap());
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
    }

    #[test]
    fn existing_directories_retain_identity_and_detect_selected_changes() {
        let root = Root::new();
        fs::create_dir_all(root.0.join("skills/example")).unwrap();
        fs::write(root.0.join("skills/example/original"), b"unaltered").unwrap();
        let before = tree(&root.0);
        let snapshot = observe_destination(&root.0, Path::new("skills/example")).unwrap();
        assert_eq!(tree(&root.0), before);
        assert_eq!(snapshot.presence(), DestinationPresence::ExistingDirectory);
        assert_eq!(snapshot.existing_directories().count(), 3);
        let identities = snapshot
            .existing_directories()
            .map(|(_, stamp)| (stamp.device, stamp.inode))
            .collect::<Vec<_>>();
        let second = observe_destination(&root.0, Path::new("skills/example")).unwrap();
        assert_eq!(
            identities,
            second
                .existing_directories()
                .map(|(_, stamp)| (stamp.device, stamp.inode))
                .collect::<Vec<_>>()
        );
        fs::write(root.0.join("unrelated"), b"caller-owned").unwrap();
        fs::write(
            root.0.join("skills/example/payload"),
            b"not compared by namespace observation",
        )
        .unwrap();
        assert!(snapshot.is_current().unwrap());
        fs::rename(root.0.join("skills/example"), root.0.join("skills/old")).unwrap();
        fs::create_dir(root.0.join("skills/example")).unwrap();
        assert!(!snapshot.is_current().unwrap());
    }

    #[test]
    fn permission_changes_invalidate_a_directory_precondition() {
        let root = Root::new();
        fs::create_dir(root.0.join("skills")).unwrap();
        let snapshot = observe_destination(&root.0, Path::new("skills/missing")).unwrap();
        fs::set_permissions(root.0.join("skills"), fs::Permissions::from_mode(0o700)).unwrap();
        // Set an actually different mode even when the process umask starts at 077.
        if snapshot.is_current().unwrap() {
            fs::set_permissions(root.0.join("skills"), fs::Permissions::from_mode(0o750)).unwrap();
        }
        assert!(!snapshot.is_current().unwrap());
    }

    #[test]
    fn root_and_descendant_aliases_or_files_do_not_become_writable_paths() {
        let root = Root::new();
        let trusted = root.0.join("trusted");
        fs::create_dir(&trusted).unwrap();
        fs::create_dir(root.0.join("outside")).unwrap();
        fs::write(root.0.join("outside/sentinel"), b"unchanged").unwrap();
        symlink(root.0.join("outside"), trusted.join("alias")).unwrap();
        for requested in [
            trusted.join("alias"),
            trusted.join("alias/"),
            trusted.join("alias/."),
        ] {
            assert_eq!(
                observe_destination(&requested, Path::new("missing"))
                    .unwrap_err()
                    .kind,
                NamespaceErrorKind::UnsafeEntry
            );
        }
        assert_eq!(
            observe_destination(&trusted, Path::new("alias/missing"))
                .unwrap_err()
                .kind,
            NamespaceErrorKind::UnsafeEntry
        );
        fs::write(trusted.join("file"), b"inert").unwrap();
        assert_eq!(
            observe_destination(&trusted, Path::new("file"))
                .unwrap_err()
                .kind,
            NamespaceErrorKind::UnsafeEntry
        );
        assert_eq!(
            observe_destination(&trusted, Path::new("file/child"))
                .unwrap_err()
                .kind,
            NamespaceErrorKind::UnsafeEntry
        );
        assert_eq!(
            fs::read(root.0.join("outside/sentinel")).unwrap(),
            b"unchanged"
        );
    }

    #[test]
    fn traversal_and_fixed_resource_limits_are_explicit() {
        let root = Root::new();
        for relative in [
            "../escape",
            "/absolute",
            "bad:name",
            "NUL.txt",
            "x/../y",
            "x\\y",
        ] {
            assert_eq!(
                observe_destination(&root.0, Path::new(relative))
                    .unwrap_err()
                    .kind,
                NamespaceErrorKind::InvalidRelativePath
            );
        }
        for invalid in [
            PathBuf::from("relative"),
            root.0.join(".."),
            root.0.join("nul\0name"),
        ] {
            assert_eq!(
                observe_destination(&invalid, Path::new("skill"))
                    .unwrap_err()
                    .kind,
                NamespaceErrorKind::InvalidRoot
            );
        }
        let path: PathBuf = std::iter::repeat_n("part", 65).collect();
        assert_eq!(
            observe_destination(&root.0, &path).unwrap_err().kind,
            NamespaceErrorKind::Limit("relative component count")
        );
        let root_missing = root.0.join(&path);
        assert_eq!(
            observe_destination(&root_missing, Path::new("skill"))
                .unwrap_err()
                .kind,
            NamespaceErrorKind::Limit("missing root component count")
        );
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
    }

    #[test]
    fn fixed_byte_and_component_boundaries_do_not_create_paths() {
        let root = Root::new();
        let relative: PathBuf = std::iter::repeat_n("part", 64).collect();
        assert_eq!(
            observe_destination(&root.0, &relative)
                .unwrap()
                .missing_components()
                .len(),
            64
        );
        assert_eq!(
            observe_destination(&root.0.join(&relative), Path::new(""))
                .unwrap()
                .missing_components()
                .len(),
            64
        );
        let too_long = "a".repeat(8193);
        assert_eq!(
            observe_destination(&root.0, Path::new(&too_long))
                .unwrap_err()
                .kind,
            NamespaceErrorKind::Limit("relative bytes")
        );
        assert_eq!(
            observe_destination(&root.0.join("a".repeat(32769)), Path::new(""))
                .unwrap_err()
                .kind,
            NamespaceErrorKind::Limit("root bytes")
        );
        let existing = observe_destination(&root.0, Path::new("")).unwrap();
        assert_eq!(existing.presence(), DestinationPresence::ExistingDirectory);
        assert_eq!(existing.existing_directories().count(), 1);
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
    }

    #[test]
    fn unicode_and_native_absent_path_identity_is_preserved() {
        let root = Root::new();
        fs::create_dir(root.0.join("home 日本語")).unwrap();
        let snapshot =
            observe_destination(&root.0.join("home 日本語"), Path::new("skills/éxample")).unwrap();
        assert_eq!(
            snapshot.missing_components(),
            ["skills", "éxample"].map(std::ffi::OsString::from)
        );
        assert!(snapshot.is_current().unwrap());
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::ffi::OsStringExt;
            let raw = std::ffi::OsString::from_vec(b"missing-\xff".to_vec());
            let snapshot = observe_destination(&root.0, Path::new(&raw)).unwrap();
            assert_eq!(snapshot.missing_components(), [raw]);
            assert!(snapshot.is_current().unwrap());
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{os::windows::fs::symlink_dir, path::Path};
    #[test]
    fn missing_prefix_observation_is_read_only_and_appearance_invalidates_absence() {
        let root = Root::new();
        let requested = root.0.join("missing home 中");
        let snapshot = observe_destination(&requested, Path::new(".agents/skills/sample")).unwrap();
        assert_eq!(snapshot.presence(), DestinationPresence::Missing);
        assert_eq!(snapshot.missing_components().len(), 4);
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
        assert!(snapshot.is_current().unwrap());
        fs::create_dir(&requested).unwrap();
        assert!(!snapshot.is_current().unwrap());
        assert!(!requested.join(".agents").exists());
    }
    #[test]
    fn existing_handles_preserve_identity_ignore_unrelated_entries_and_detect_read_only_change() {
        let root = Root::new();
        let trusted = root.0.join("home with spaces 中");
        fs::create_dir_all(trusted.join("child")).unwrap();
        let snapshot = observe_destination(&trusted, Path::new("child")).unwrap();
        assert_eq!(snapshot.presence(), DestinationPresence::ExistingDirectory);
        let identities = snapshot
            .existing_directories()
            .map(|(_, stamp)| stamp.file_identity())
            .collect::<Vec<_>>();
        assert_eq!(identities.len(), 2);
        assert_ne!(identities[0], identities[1]);
        fs::write(trusted.join("unrelated"), b"inert").unwrap();
        assert!(snapshot.is_current().unwrap());
        assert!(fs::rename(trusted.join("child"), trusted.join("moved")).is_err());
        let original_permissions = fs::metadata(trusted.join("child")).unwrap().permissions();
        let mut permissions = original_permissions.clone();
        permissions.set_readonly(true);
        fs::set_permissions(trusted.join("child"), permissions).unwrap();
        assert!(!snapshot.is_current().unwrap());
        fs::set_permissions(trusted.join("child"), original_permissions).unwrap();
    }
    #[test]
    fn native_reparse_points_and_unsafe_components_do_not_become_destinations() {
        let root = Root::new();
        let trusted = root.0.join("trusted");
        let outside = root.0.join("outside");
        fs::create_dir(&trusted).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"untouched").unwrap();
        for relative in ["../outside", r"C:\escape", r"child:stream", "NUL.txt"] {
            assert!(observe_destination(&trusted, Path::new(relative)).is_err());
        }
        symlink_dir(&outside, trusted.join("link"))
            .expect("required Windows reparse fixture privilege");
        assert_eq!(
            observe_destination(&trusted, Path::new("link/new"))
                .unwrap_err()
                .kind,
            NamespaceErrorKind::UnsafeEntry
        );
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"untouched");
        assert!(!outside.join("new").exists());
        fs::remove_dir(trusted.join("link")).unwrap();
    }
}

#[cfg(windows)]
#[test]
fn windows_acl_inheritance_change_invalidates_a_selected_directory_precondition() {
    use std::{
        fs::OpenOptions,
        os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
        path::Path,
        ptr,
    };
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{GetSecurityInfo, SE_FILE_OBJECT, SetSecurityInfo},
            DACL_SECURITY_INFORMATION, GetSecurityDescriptorControl, GetSecurityDescriptorDacl,
            PROTECTED_DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED,
            UNPROTECTED_DACL_SECURITY_INFORMATION,
        },
        Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            FILE_SHARE_WRITE, READ_CONTROL, WRITE_DAC,
        },
    };
    let root = Root::new();
    let selected = root.0.join("selected");
    fs::create_dir(&selected).unwrap();
    let snapshot = observe_destination(&root.0, Path::new("selected")).unwrap();
    assert!(snapshot.is_current().unwrap());
    let writer = OpenOptions::new()
        .access_mode(READ_CONTROL | WRITE_DAC)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&selected)
        .unwrap();
    let mut raw = ptr::null_mut();
    // SAFETY: retained fixture-only handle and one allocated descriptor output.
    assert_eq!(
        unsafe {
            GetSecurityInfo(
                writer.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut raw,
            )
        },
        0
    );
    assert!(!raw.is_null());
    struct Descriptor(*mut core::ffi::c_void);
    impl Drop for Descriptor {
        fn drop(&mut self) {
            // SAFETY: allocation returned by successful GetSecurityInfo, freed once.
            unsafe {
                LocalFree(self.0);
            }
        }
    }
    let descriptor = Descriptor(raw);
    let mut present = 0;
    let mut defaulted = 0;
    let mut acl = ptr::null_mut();
    let mut control = 0;
    let mut revision = 0;
    // SAFETY: valid API-owned descriptor, typed writable outputs. ACL remains
    // borrowed from the live descriptor for both fixture-only mutations below.
    unsafe {
        assert_ne!(
            GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut acl, &mut defaulted),
            0
        );
        assert_ne!(
            GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision),
            0
        );
    }
    assert_ne!(present, 0);
    assert!(
        !acl.is_null(),
        "fixture must have a real DACL; never install a null DACL"
    );
    let protected = control & SE_DACL_PROTECTED != 0;
    let original = if protected {
        PROTECTED_DACL_SECURITY_INFORMATION
    } else {
        UNPROTECTED_DACL_SECURITY_INFORMATION
    };
    let changed = if protected {
        UNPROTECTED_DACL_SECURITY_INFORMATION
    } else {
        PROTECTED_DACL_SECURITY_INFORMATION
    };
    // SAFETY: only toggles inheritance on an owned empty fixture directory; the
    // exact original ACL is supplied, not a null/everyone-full-access replacement.
    assert_eq!(
        unsafe {
            SetSecurityInfo(
                writer.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | changed,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null_mut(),
            )
        },
        0
    );
    let current = snapshot.is_current();
    // Restore before asserting, including if revalidation returned an error.
    // SAFETY: same live writer/descriptor; original ACL and inheritance mode.
    assert_eq!(
        unsafe {
            SetSecurityInfo(
                writer.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | original,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null_mut(),
            )
        },
        0
    );
    assert!(
        !current.unwrap(),
        "ACL-only changes must not pass a stale namespace precondition"
    );
}
