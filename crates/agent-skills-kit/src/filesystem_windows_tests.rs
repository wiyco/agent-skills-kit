use super::*;
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "ask-windows-handles-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
                Err(error) => panic!("owned native fixture unavailable: {error}"),
            }
        }
        panic!("fixture collision budget")
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("owned Windows fixture teardown");
    }
}

#[test]
fn native_components_reject_traversal_streams_devices_and_preserve_wide_identity() {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    for bad in [
        "",
        ".",
        "..",
        "../escape",
        "C:relative",
        "resource:stream",
        "a/b",
        "a\\b",
        "a.",
        "a ",
        "NUL.txt",
        "com1",
        "lpt²",
        "CON",
        "a\0b",
    ] {
        assert!(component(OsStr::new(bad)).is_err(), "{bad:?}");
    }
    let original = vec![b'r' as u16, 0xd800];
    assert_eq!(
        component(&OsString::from_wide(&original)).unwrap(),
        original
    );
}

#[test]
fn native_relative_creation_reads_exact_bytes_and_exclusive_commit_never_replaces() {
    let root = Root::new();
    let parent = open_root(&root.0).unwrap();
    for winner in ["absent", "file", "empty-directory", "link"] {
        let stage = child(&parent, OsStr::new("stage"), Kind::Directory, true).unwrap();
        let mut resource = child(
            &stage,
            OsStr::new("reference café 中.txt"),
            Kind::File,
            true,
        )
        .unwrap();
        resource.write_all(b"inert payload bytes").unwrap();
        let identity = stamp(&resource).unwrap();
        assert_eq!(identity.length, b"inert payload bytes".len() as i64);
        assert_eq!(identity.links, 1);
        assert!(
            child(
                &stage,
                OsStr::new("reference café 中.txt"),
                Kind::File,
                true
            )
            .is_err()
        );
        drop(resource);
        let mut read = child(
            &stage,
            OsStr::new("reference café 中.txt"),
            Kind::File,
            false,
        )
        .unwrap();
        let mut bytes = Vec::new();
        read.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"inert payload bytes");
        assert_eq!(stamp(&read).unwrap().identity, identity.identity);
        assert_eq!(stamp(&read).unwrap().length, identity.length);
        drop(read);
        let destination = root.0.join("destination");
        let outside = root.0.join("outside");
        match winner {
            "file" => fs::write(&destination, b"winner").unwrap(),
            "empty-directory" => fs::create_dir(&destination).unwrap(),
            "link" => {
                fs::create_dir(&outside).unwrap();
                fs::write(outside.join("sentinel"), b"unrelated").unwrap();
                std::os::windows::fs::symlink_dir(&outside, &destination).expect(
                    "required Windows symlink fixture privilege; not a skipped passing test",
                );
            }
            _ => (),
        }
        let before = stamp(&stage).unwrap();
        let result = rename_exclusive(&stage, &parent, OsStr::new("destination"));
        if winner == "absent" {
            result.unwrap();
            assert_eq!(
                fs::read(destination.join("reference café 中.txt")).unwrap(),
                b"inert payload bytes"
            );
            assert!(!root.0.join("stage").exists());
            assert_eq!(stamp(&stage).unwrap().identity, before.identity);
        } else {
            assert!(result.is_err(), "{winner} must survive native commit");
            assert_eq!(stamp(&stage).unwrap(), before);
            match winner {
                "file" => assert_eq!(fs::read(&destination).unwrap(), b"winner"),
                "empty-directory" => assert_eq!(fs::read_dir(&destination).unwrap().count(), 0),
                "link" => assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"unrelated"),
                _ => unreachable!(),
            }
        }
        drop(stage);
        if winner == "link" {
            fs::remove_dir(&destination).unwrap();
            fs::remove_dir_all(outside).unwrap();
        } else if winner == "file" {
            fs::remove_file(&destination).unwrap();
        } else {
            fs::remove_dir_all(&destination).unwrap();
        }
        if root.0.join("stage").exists() {
            fs::remove_dir_all(root.0.join("stage")).unwrap();
        }
    }
}

#[test]
fn retained_parent_blocks_rename_and_reparse_or_hardlink_inputs_are_rejected() {
    let root = Root::new();
    let directory = root.0.join("parent");
    fs::create_dir(&directory).unwrap();
    let held = open_root(&directory).unwrap();
    assert!(fs::rename(&directory, root.0.join("moved")).is_err());
    let outside = root.0.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"outside").unwrap();
    let link = directory.join("link");
    std::os::windows::fs::symlink_dir(&outside, &link)
        .expect("required real reparse fixture privilege");
    assert!(child(&held, OsStr::new("link"), Kind::Directory, false).is_err());
    assert!(open_root(&link).is_err());
    fs::write(directory.join("original"), b"multiply linked").unwrap();
    fs::hard_link(directory.join("original"), directory.join("alias")).unwrap();
    assert!(child(&held, OsStr::new("alias"), Kind::File, false).is_err());
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"outside");
    fs::remove_dir(link).unwrap();
    drop(held);
    fs::rename(&directory, root.0.join("moved")).unwrap();
}

#[test]
fn handle_cleanup_removes_only_the_created_empty_object() {
    let root = Root::new();
    let parent = open_root(&root.0).unwrap();
    fs::write(root.0.join("unrelated"), b"preserve").unwrap();
    let owned = child(&parent, OsStr::new("owned"), Kind::Directory, true).unwrap();
    let payload = child(&owned, OsStr::new("resource"), Kind::File, true).unwrap();
    assert!(fs::rename(root.0.join("owned"), root.0.join("substitute")).is_err());
    delete_owned(payload).unwrap();
    assert!(!root.0.join("owned/resource").exists());
    delete_owned(owned).unwrap();
    assert!(!root.0.join("owned").exists());
    assert_eq!(fs::read(root.0.join("unrelated")).unwrap(), b"preserve");
}
