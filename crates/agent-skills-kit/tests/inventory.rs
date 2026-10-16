use agent_skills_kit::validation::*;
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
                "ask-inventory-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create owned test root: {error}"),
            }
        }
        panic!("could not exclusively create temporary root")
    }
    fn source(&self) -> PathBuf {
        let path = self.0.join("sample");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("SKILL.md"), b"---\nname: sample\ndescription: Inert original fixture\ncustom: keep-me\n---\nNever execute: touch forbidden\n").unwrap();
        path
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("clean owned root");
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[test]
fn unsupported_native_inventory_fails_closed_without_inspection() {
    let root = Root::new();
    let source = root.source();
    assert_eq!(
        inventory_skill(&source, InventoryLimits::default())
            .unwrap_err()
            .kind,
        InventoryErrorKind::UnsupportedPlatform
    );
    assert_eq!(
        inventory_skill(&root.0.join("absent"), InventoryLimits::default())
            .unwrap_err()
            .kind,
        InventoryErrorKind::UnsupportedPlatform
    );
    assert!(!root.0.join("forbidden").exists());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod native {
    use super::*;
    #[cfg(target_os = "linux")]
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::{
        ffi::OsStringExt,
        fs::{MetadataExt, PermissionsExt, symlink},
    };
    use std::path::Path;

    fn snapshot(path: &Path) -> Vec<(PathBuf, Vec<u8>, u32)> {
        fn walk(root: &Path, relative: &Path, out: &mut Vec<(PathBuf, Vec<u8>, u32)>) {
            let path = root.join(relative);
            let metadata = fs::symlink_metadata(&path).unwrap();
            let bytes = if metadata.is_file() {
                fs::read(&path).unwrap()
            } else {
                Vec::new()
            };
            out.push((relative.to_owned(), bytes, metadata.mode()));
            if metadata.is_dir() {
                for entry in fs::read_dir(path).unwrap() {
                    walk(root, &relative.join(entry.unwrap().file_name()), out);
                }
            }
        }
        let mut result = Vec::new();
        walk(path, Path::new(""), &mut result);
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    #[test]
    fn complete_inventory_is_read_only_deterministic_and_preserves_resources() {
        let root = Root::new();
        let source = root.source();
        fs::create_dir_all(source.join("assets/empty")).unwrap();
        fs::write(source.join("assets/binary"), [0, 255, 128, 0]).unwrap();
        fs::write(source.join("script.sh"), b"#!/bin/sh\ntouch forbidden\n").unwrap();
        fs::set_permissions(source.join("script.sh"), fs::Permissions::from_mode(0o6755)).unwrap();
        fs::write(source.join("private"), b"abc").unwrap();
        fs::set_permissions(source.join("private"), fs::Permissions::from_mode(0o600)).unwrap();
        let before = snapshot(&root.0);
        let first = inventory_skill(&source, InventoryLimits::default()).unwrap();
        let second = inventory_skill(&source, InventoryLimits::default()).unwrap();
        assert_eq!(snapshot(&root.0), before);
        assert_eq!(first.inventory().entries(), second.inventory().entries());
        assert_eq!(
            first.inventory().canonical_root(),
            fs::canonicalize(&source).unwrap()
        );
        assert_eq!(first.inventory().requested_root(), source);
        assert!(first.inventory().root_is_unchanged().unwrap());
        let entries = first.inventory().entries();
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.relative_path.as_path())
                .collect::<Vec<_>>(),
            [
                "SKILL.md",
                "assets",
                "assets/binary",
                "assets/empty",
                "private",
                "script.sh"
            ]
            .map(Path::new)
        );
        let private = entries
            .iter()
            .find(|entry| entry.relative_path == Path::new("private"))
            .unwrap();
        assert_eq!(private.output_mode, 0o600);
        assert_eq!(private.digest.unwrap(), *blake3::hash(b"abc").as_bytes());
        let script = entries
            .iter()
            .find(|entry| entry.relative_path == Path::new("script.sh"))
            .unwrap();
        assert_eq!(script.output_mode, 0o755);
        assert_eq!(script.output_mode & !script.stamp.mode, 0);
        assert!(
            entries
                .iter()
                .filter(|entry| entry.kind == SourceEntryKind::Directory)
                .all(|entry| entry.digest.is_none())
        );
        assert_eq!(
            first.metadata().source_bytes(),
            fs::read(source.join("SKILL.md")).unwrap()
        );
        assert_eq!(
            first.inventory().total_bytes(),
            entries
                .iter()
                .filter(|entry| entry.kind == SourceEntryKind::File)
                .map(|entry| entry.stamp.length)
                .sum::<u64>()
        );
        assert!(!source.join("forbidden").exists());
        assert!(!root.0.join("forbidden").exists());
    }

    #[test]
    fn large_file_digest_covers_all_chunks() {
        let root = Root::new();
        let source = root.source();
        let bytes = (0..65537)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        fs::write(source.join("resource"), &bytes).unwrap();
        let skill = inventory_skill(&source, InventoryLimits::default()).unwrap();
        let file = skill
            .inventory()
            .entries()
            .iter()
            .find(|entry| entry.relative_path == Path::new("resource"))
            .unwrap();
        assert_eq!(file.stamp.length, bytes.len() as u64);
        assert_eq!(file.digest.unwrap(), *blake3::hash(&bytes).as_bytes());
    }

    #[test]
    fn unicode_resource_and_source_names_are_retained() {
        let root = Root::new();
        let source = root.source();
        fs::write(source.join("asset 日本語"), b"data").unwrap();
        let skill = inventory_skill(&source, InventoryLimits::default()).unwrap();
        assert!(
            skill
                .inventory()
                .entries()
                .iter()
                .any(|entry| entry.relative_path == Path::new("asset 日本語"))
        );
        let named = root.0.join("日本語");
        fs::create_dir(&named).unwrap();
        fs::write(
            named.join("SKILL.md"),
            "---\nname: 日本語\ndescription: fixture\n---\n",
        )
        .unwrap();
        assert_eq!(
            inventory_skill(&named, InventoryLimits::default())
                .unwrap()
                .metadata()
                .name(),
            "日本語"
        );
    }

    #[test]
    fn native_non_utf8_resource_capability_is_explicit() {
        let root = Root::new();
        let source = root.source();
        let raw = std::ffi::OsString::from_vec(b"resource-\xff".to_vec());
        #[cfg(target_os = "linux")]
        {
            fs::write(source.join(&raw), b"inert").unwrap();
            let skill = inventory_skill(&source, InventoryLimits::default()).unwrap();
            assert!(
                skill.inventory().entries().iter().any(|entry| entry
                    .relative_path
                    .as_os_str()
                    .as_bytes()
                    == raw.as_bytes())
            );
        }
        #[cfg(target_os = "macos")]
        {
            // APFS requires valid UTF-8 names. This is native rejection
            // evidence, not a skipped claim of filename-preservation coverage.
            let error = fs::write(source.join(&raw), b"inert").unwrap_err();
            assert_eq!(error.raw_os_error(), Some(libc::EILSEQ));
            assert!(inventory_skill(&source, InventoryLimits::default()).is_ok());
        }
    }

    #[test]
    fn descendant_links_dangling_links_and_hardlinks_are_rejected() {
        for link in ["file", "directory", "dangling"] {
            let root = Root::new();
            let source = root.source();
            let outside = root.0.join("outside");
            fs::create_dir(&outside).unwrap();
            fs::write(outside.join("sentinel"), b"unchanged").unwrap();
            let destination = match link {
                "file" => outside.join("sentinel"),
                "directory" => outside.clone(),
                _ => outside.join("absent"),
            };
            symlink(destination, source.join("link")).unwrap();
            let error = inventory_skill(&source, InventoryLimits::default()).unwrap_err();
            assert_eq!(error.kind, InventoryErrorKind::UnsafeEntry);
            assert_eq!(error.path, source.join("link"));
            assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"unchanged");
        }
        let root = Root::new();
        let source = root.source();
        fs::hard_link(source.join("SKILL.md"), source.join("alias")).unwrap();
        assert_eq!(
            inventory_skill(&source, InventoryLimits::default())
                .unwrap_err()
                .kind,
            InventoryErrorKind::HardLinkedFile
        );
    }

    #[test]
    fn fifo_and_socket_are_rejected_without_reading_or_blocking() {
        let root = Root::new();
        let source = root.source();
        // Fixed system fixture utility; no command comes from skill contents.
        assert!(
            std::process::Command::new("/usr/bin/mkfifo")
                .arg(source.join("pipe"))
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(
            inventory_skill(&source, InventoryLimits::default())
                .unwrap_err()
                .kind,
            InventoryErrorKind::UnsafeEntry
        );
        fs::remove_file(source.join("pipe")).unwrap();
        let _socket = std::os::unix::net::UnixListener::bind(source.join("socket")).unwrap();
        assert_eq!(
            inventory_skill(&source, InventoryLimits::default())
                .unwrap_err()
                .kind,
            InventoryErrorKind::UnsafeEntry
        );
    }

    #[test]
    fn unsafe_resource_names_are_not_silently_excluded() {
        for name in [
            "bad:name",
            "bad\\name",
            "bad?name",
            "trailing.",
            "trailing ",
            "CON.txt",
            "NUL  .txt",
            "LPT³.log",
            "control\u{85}",
        ] {
            let root = Root::new();
            let source = root.source();
            fs::write(source.join(name), b"inert").unwrap();
            let error = inventory_skill(&source, InventoryLimits::default()).unwrap_err();
            assert_eq!(
                error.kind,
                InventoryErrorKind::UnsafeName,
                "accepted {name:?}"
            );
            assert_eq!(error.path, source.join(name));
        }
    }

    #[test]
    fn root_aliases_and_implicit_roots_are_not_followed() {
        let root = Root::new();
        let source = root.source();
        let alias = root.0.join("alias");
        symlink(&source, &alias).unwrap();
        for path in [alias.clone(), alias.join(""), alias.join(".")] {
            assert!(
                inventory_skill(&path, InventoryLimits::default()).is_err(),
                "followed {path:?}"
            );
        }
        for path in [
            PathBuf::from("relative"),
            source.join(".."),
            source.join("nul\0name"),
        ] {
            assert_eq!(
                inventory_skill(&path, InventoryLimits::default())
                    .unwrap_err()
                    .kind,
                InventoryErrorKind::InvalidRoot
            );
        }
        let raw = root
            .0
            .join(std::ffi::OsString::from_vec(b"skill-\xff".to_vec()));
        assert_eq!(
            inventory_skill(&raw, InventoryLimits::default())
                .unwrap_err()
                .kind,
            InventoryErrorKind::NonUtf8SkillName
        );
    }

    #[test]
    fn budgets_fail_closed_at_exact_boundaries() {
        let root = Root::new();
        let source = root.source();
        let manifest_length = fs::metadata(source.join("SKILL.md")).unwrap().len();
        let limits = InventoryLimits {
            max_entries: 1,
            max_file_bytes: manifest_length,
            max_total_bytes: manifest_length,
            max_depth: 1,
            max_path_bytes: 8,
            ..InventoryLimits::default()
        };
        assert!(inventory_skill(&source, limits).is_ok());
        for (limits, expected) in [
            (
                InventoryLimits {
                    max_file_bytes: manifest_length - 1,
                    ..limits
                },
                "file bytes",
            ),
            (
                InventoryLimits {
                    max_total_bytes: manifest_length - 1,
                    ..limits
                },
                "total bytes",
            ),
            (
                InventoryLimits {
                    max_path_bytes: 7,
                    ..limits
                },
                "relative path bytes",
            ),
        ] {
            assert_eq!(
                inventory_skill(&source, limits).unwrap_err().kind,
                InventoryErrorKind::Limit(expected)
            );
        }
        fs::create_dir(source.join("assets")).unwrap();
        assert_eq!(
            inventory_skill(&source, limits).unwrap_err().kind,
            InventoryErrorKind::Limit("entry count")
        );
        fs::write(source.join("assets/file"), b"x").unwrap();
        assert_eq!(
            inventory_skill(
                &source,
                InventoryLimits {
                    max_depth: 1,
                    ..InventoryLimits::default()
                }
            )
            .unwrap_err()
            .kind,
            InventoryErrorKind::Limit("entry depth")
        );
        assert_eq!(
            inventory_skill(
                &source,
                InventoryLimits {
                    max_entries: 0,
                    ..InventoryLimits::default()
                }
            )
            .unwrap_err()
            .kind,
            InventoryErrorKind::InvalidLimits
        );
    }

    #[test]
    fn missing_or_invalid_manifest_returns_a_typed_error_without_changes() {
        let root = Root::new();
        let source = root.source();
        fs::remove_file(source.join("SKILL.md")).unwrap();
        assert_eq!(
            inventory_skill(&source, InventoryLimits::default())
                .unwrap_err()
                .kind,
            InventoryErrorKind::MissingSkillMd
        );
        fs::write(
            source.join("SKILL.md"),
            b"---\nname: different\ndescription: inert\n---\n",
        )
        .unwrap();
        let before = snapshot(&root.0);
        assert!(matches!(
            inventory_skill(&source, InventoryLimits::default())
                .unwrap_err()
                .kind,
            InventoryErrorKind::Metadata(Diagnostic {
                kind: DiagnosticKind::DirectoryNameMismatch,
                ..
            })
        ));
        assert_eq!(snapshot(&root.0), before);
    }
}

#[cfg(windows)]
#[test]
fn windows_real_directory_reparse_fixture_is_refused_without_touching_target() {
    use std::os::windows::fs::{MetadataExt, symlink_dir};
    let root = Root::new();
    let source = root.source();
    let outside = root.0.join("outside with spaces 中");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"unrelated original bytes").unwrap();
    let link = source.join("directory-link");
    symlink_dir(&outside, &link).unwrap_or_else(|error| {
        panic!("required native Windows reparse fixture unavailable: {error}; link privilege/Developer Mode must be available; this is not a passing support test")
    });
    assert_ne!(
        fs::symlink_metadata(&link).unwrap().file_attributes() & 0x400,
        0,
        "fixture must actually carry FILE_ATTRIBUTE_REPARSE_POINT"
    );
    assert_eq!(
        inventory_skill(&source, InventoryLimits::default())
            .unwrap_err()
            .kind,
        InventoryErrorKind::UnsupportedPlatform
    );
    assert_eq!(
        inventory_skill(&link, InventoryLimits::default())
            .unwrap_err()
            .kind,
        InventoryErrorKind::UnsupportedPlatform
    );
    assert_eq!(
        fs::read(outside.join("sentinel")).unwrap(),
        b"unrelated original bytes"
    );
    assert_eq!(
        fs::canonicalize(&link).unwrap(),
        fs::canonicalize(&outside).unwrap()
    );
    // Remove the directory link itself before ordinary owned-root teardown.
    fs::remove_dir(&link).unwrap();
    assert!(outside.join("sentinel").is_file());
}
