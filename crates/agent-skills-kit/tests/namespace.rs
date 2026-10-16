use agent_skills_kit::planner::*;
use std::{
    fs,
    path::{Path, PathBuf},
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

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
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
