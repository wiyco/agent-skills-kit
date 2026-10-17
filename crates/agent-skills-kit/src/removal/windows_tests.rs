use super::*;
use crate::{
    catalog::{Catalog, Scope},
    discovery::{DiscoveryLimits, list_skills},
    removal::{RemovalLimits, RemovalRequest, apply_removal, plan_removal},
    resolver::{InstallContext, native_platform},
};
use std::{
    fs,
    os::windows::fs::{MetadataExt, OpenOptionsExt, symlink_dir},
};
use windows_sys::Win32::{
    Foundation::ERROR_SHARING_VIOLATION,
    Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    },
};
struct Fixture {
    root: PathBuf,
    home: PathBuf,
    source: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let mut created = None;
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "ask-remove-windows-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => {
                    created = Some(fs::canonicalize(path).unwrap());
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("owned fixture: {e}"),
            }
        }
        let root = created.expect("owned fixture collision budget");
        let home = root.join("home");
        let source = home.join(".agents/skills/sample");
        fs::create_dir_all(source.join("resources/empty")).unwrap();
        fs::write(source.join("SKILL.md"),b"---\nname: sample\ndescription: Original inert Windows removal fixture.\n---\nNever execute.\n").unwrap();
        fs::write(source.join("resources/a.txt"), b"original A").unwrap();
        fs::write(source.join("resources/b.txt"), b"original B").unwrap();
        fs::write(
            root.join("outside-sentinel"),
            b"unrelated original sentinel",
        )
        .unwrap();
        Self { root, home, source }
    }
    fn context(&self) -> InstallContext {
        let mut context = InstallContext::new(native_platform().unwrap());
        context.user_home = Some(self.home.clone());
        context
    }
    fn listing(&self) -> crate::discovery::SkillListing {
        list_skills(
            &Catalog::bundled().unwrap(),
            "codex",
            Scope::User,
            &self.context(),
            DiscoveryLimits::default(),
        )
        .unwrap()
    }
    fn plan(&self) -> RemovalPlan {
        let listing = self.listing();
        let location_index = listing
            .locations
            .iter()
            .position(|location| location.preferred_write)
            .unwrap();
        plan_removal(
            &Catalog::bundled().unwrap(),
            RemovalRequest {
                listing,
                location_index,
                skill_name: "sample".into(),
                context: self.context(),
                limits: RemovalLimits::default(),
            },
        )
        .unwrap()
    }
    fn sentinel(&self) {
        assert_eq!(
            fs::read(self.root.join("outside-sentinel")).unwrap(),
            b"unrelated original sentinel"
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fn restore(path: &Path) {
            let metadata = fs::symlink_metadata(path).unwrap();
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return;
            }
            if metadata.is_dir() {
                for entry in fs::read_dir(path).unwrap() {
                    restore(&entry.unwrap().path());
                }
            } else if metadata.permissions().readonly() {
                let mut permission = metadata.permissions();
                permission.set_readonly(false);
                fs::set_permissions(path, permission).unwrap();
            }
        }
        restore(&self.root);
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn approved(plan: RemovalPlan) -> crate::removal::ApprovedRemoval {
    let token = plan.confirmation_token().to_owned();
    plan.approve_permanent_removal(&token).unwrap()
}
fn run(plan: &RemovalPlan, hook: &mut impl FnMut(Phase, &Path) -> io::Result<()>) -> RemovalReport {
    let mut report = super::super::initial(plan);
    apply_with_hook(plan, &mut report, hook);
    report
}
fn removed(report: &RemovalReport) -> usize {
    report
        .outcomes
        .iter()
        .filter(|outcome| outcome.state == RemovalEntryState::Removed)
        .count()
}
#[test]
fn actual_ntfs_permanent_removal_closes_handles_and_leaves_no_payload_or_enclosure() {
    let f = Fixture::new();
    let plan = f.plan();
    let total = plan.entries().len() + 1;
    let report = apply_removal(approved(plan));
    assert!(report.is_successful(), "{report:?}");
    assert!(report.permanent_removal_attempted);
    assert_eq!(removed(&report), total);
    assert_eq!(
        fs::symlink_metadata(&f.source).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(fs::read_dir(f.source.parent().unwrap()).unwrap().count(), 0);
    f.sentinel();
}
#[test]
fn stale_bytes_and_new_hardlinks_fail_read_only_before_detachment() {
    for hardlink in [false, true] {
        let f = Fixture::new();
        let plan = f.plan();
        if hardlink {
            fs::hard_link(
                f.source.join("resources/a.txt"),
                f.root.join("outside-hardlink"),
            )
            .unwrap();
        } else {
            fs::write(f.source.join("resources/a.txt"), b"edited before apply").unwrap();
        }
        let report = apply_removal(approved(plan));
        assert!(!report.is_successful());
        assert_eq!(removed(&report), 0);
        assert!(!report.permanent_removal_attempted);
        assert!(report.retained_stage.is_none());
        assert!(f.source.join("SKILL.md").is_file());
        if hardlink {
            assert_eq!(
                fs::read(f.root.join("outside-hardlink")).unwrap(),
                b"original A"
            );
        }
        f.sentinel();
    }
}
#[test]
fn independent_discovery_reader_handles_refuse_delete_access_without_force() {
    let f = Fixture::new();
    let readers = f.listing();
    let plan = f.plan();
    let report = apply_removal(approved(plan));
    assert_eq!(removed(&report), 0);
    assert!(!report.permanent_removal_attempted);
    assert!(report.retained_stage.is_none());
    assert!(
        matches!(report.failure.as_ref().map(|failure|&failure.kind),Some(RemovalFailureKind::Io {raw_os_error:Some(code),..}) if *code==ERROR_SHARING_VIOLATION as i32),
        "{report:?}"
    );
    assert_eq!(
        fs::read(f.source.join("resources/a.txt")).unwrap(),
        b"original A"
    );
    drop(readers);
    f.sentinel();
}
#[test]
fn a_root_reparse_substitution_before_delete_open_never_reads_or_deletes_its_target() {
    let f = Fixture::new();
    let plan = f.plan();
    let saved = f.root.join("saved-approved");
    let outside = f.root.join("outside-directory");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"outside untouched").unwrap();
    let report = run(&plan, &mut |phase, _| {
        if phase == Phase::BeforeRootOpen {
            fs::rename(&f.source, &saved)?;
            symlink_dir(&outside, &f.source)
                .expect("required actual Windows reparse fixture privilege");
        }
        Ok(())
    });
    assert_eq!(removed(&report), 0);
    assert!(!report.permanent_removal_attempted);
    assert!(report.retained_stage.is_none());
    assert_eq!(
        fs::read(outside.join("sentinel")).unwrap(),
        b"outside untouched"
    );
    assert_eq!(
        fs::read(saved.join("resources/a.txt")).unwrap(),
        b"original A"
    );
    f.sentinel();
}
#[test]
fn held_delete_root_blocks_last_moment_external_rename_but_our_native_removal_succeeds() {
    let f = Fixture::new();
    let plan = f.plan();
    let saved = f.root.join("unapproved-rename");
    let mut blocked = false;
    let report = run(&plan, &mut |phase, _| {
        if phase == Phase::BeforeDetach {
            let error = fs::rename(&f.source, &saved).unwrap_err();
            assert_eq!(error.raw_os_error(), Some(ERROR_SHARING_VIOLATION as i32));
            blocked = true;
        }
        Ok(())
    });
    assert!(blocked);
    assert!(report.is_successful(), "{report:?}");
    assert!(!saved.exists());
    f.sentinel();
}
#[test]
fn changed_detached_contents_are_restored_before_any_permanent_call() {
    let f = Fixture::new();
    let plan = f.plan();
    let report = run(&plan, &mut |phase, path| {
        if phase == Phase::AfterDetach {
            fs::write(path.join("resources/a.txt"), b"edited after detach")?;
        }
        Ok(())
    });
    assert_eq!(removed(&report), 0);
    assert!(!report.permanent_removal_attempted);
    assert!(report.restored_before_removal, "{report:?}");
    assert!(report.retained_stage.is_none());
    assert_eq!(
        fs::read(f.source.join("resources/a.txt")).unwrap(),
        b"edited after detach"
    );
    f.sentinel();
}
#[test]
fn concurrent_original_name_occupants_survive_failed_native_restoration() {
    let f = Fixture::new();
    let plan = f.plan();
    let report = run(&plan, &mut |phase, path| {
        if phase == Phase::AfterDetach {
            fs::write(path.join("resources/a.txt"), b"edited retained data")?;
            fs::create_dir(&f.source)?;
            fs::write(f.source.join("winner"), b"concurrent winner")?;
        }
        Ok(())
    });
    assert_eq!(removed(&report), 0);
    assert!(!report.permanent_removal_attempted);
    assert!(!report.restored_before_removal);
    assert!(!report.cleanup_failures.is_empty());
    assert_eq!(
        fs::read(f.source.join("winner")).unwrap(),
        b"concurrent winner"
    );
    assert_eq!(
        fs::read(
            report
                .retained_stage
                .as_ref()
                .unwrap()
                .join("payload/resources/a.txt")
        )
        .unwrap(),
        b"edited retained data"
    );
    f.sentinel();
}
#[test]
fn native_no_replace_detachment_preserves_an_unknown_private_payload() {
    let f = Fixture::new();
    let plan = f.plan();
    let report = run(&plan, &mut |phase, path| {
        if phase == Phase::AfterStage {
            fs::create_dir(path.join("payload"))?;
            fs::write(path.join("payload/unapproved"), b"unknown private occupant")?;
        }
        Ok(())
    });
    assert_eq!(removed(&report), 0);
    assert!(!report.permanent_removal_attempted);
    assert!(!report.restored_before_removal);
    assert_eq!(
        fs::read(f.source.join("resources/a.txt")).unwrap(),
        b"original A"
    );
    assert_eq!(
        fs::read(
            report
                .retained_stage
                .as_ref()
                .unwrap()
                .join("payload/unapproved")
        )
        .unwrap(),
        b"unknown private occupant"
    );
    f.sentinel();
}
#[test]
fn actual_parent_dacl_denial_prevents_private_creation_without_detaching_data() {
    use native::test_support::{fixture_dacl, set_fixture_dacl};
    let f = Fixture::new();
    let parent = f.source.parent().unwrap().to_owned();
    let original = fixture_dacl(&parent);
    struct Restore(PathBuf, String);
    impl Drop for Restore {
        fn drop(&mut self) {
            set_fixture_dacl(&self.0, &self.1);
        }
    }
    let guard = Restore(parent.clone(), original);
    let sid = creation::Token::effective().unwrap().user_sid().unwrap();
    set_fixture_dacl(&parent, &format!("D:P(D;;0x4;;;WD)(A;;FA;;;{sid})"));
    let report = apply_removal(approved(f.plan()));
    assert_eq!(removed(&report), 0);
    assert!(!report.permanent_removal_attempted);
    assert!(report.retained_stage.is_none());
    assert!(
        matches!(
            report.failure.as_ref().map(|failure| &failure.kind),
            Some(RemovalFailureKind::Io {
                kind: io::ErrorKind::PermissionDenied,
                ..
            })
        ),
        "{report:?}"
    );
    assert_eq!(
        fs::read(f.source.join("resources/a.txt")).unwrap(),
        b"original A"
    );
    drop(guard);
    f.sentinel();
}
#[test]
fn readonly_refusal_keeps_partial_outcomes_and_never_clears_the_attribute() {
    let f = Fixture::new();
    let denied = f.source.join("resources/zz-readonly");
    fs::write(&denied, b"approved readonly data").unwrap();
    let mut permission = fs::metadata(&denied).unwrap().permissions();
    permission.set_readonly(true);
    fs::set_permissions(&denied, permission).unwrap();
    let report = apply_removal(approved(f.plan()));
    assert!(!report.is_successful());
    assert!(report.permanent_removal_attempted);
    assert!(removed(&report) > 0, "{report:?}");
    assert!(
        report
            .outcomes
            .iter()
            .any(|row| row.state == RemovalEntryState::Remaining),
        "{report:?}"
    );
    let retained = report
        .retained_stage
        .as_ref()
        .unwrap()
        .join("payload/resources/zz-readonly");
    assert!(fs::metadata(&retained).unwrap().permissions().readonly());
    assert_eq!(fs::read(retained).unwrap(), b"approved readonly data");
    f.sentinel();
}
#[test]
fn controlled_failure_after_first_confirmed_removal_retains_all_progress() {
    let f = Fixture::new();
    let plan = f.plan();
    let mut stopped = false;
    let report = run(&plan, &mut |phase, _| {
        if phase == Phase::AfterUnlink && !stopped {
            stopped = true;
            return Err(io::Error::other("controlled post-removal failure"));
        }
        Ok(())
    });
    assert_eq!(removed(&report), 1);
    assert!(report.permanent_removal_attempted);
    assert!(
        report
            .outcomes
            .iter()
            .any(|row| row.state == RemovalEntryState::NotAttempted)
    );
    assert!(report.retained_stage.is_some());
    assert!(!report.restored_before_removal);
    f.sentinel();
}
#[test]
fn source_named_streams_after_approval_cannot_be_adopted_for_removal() {
    let f = Fixture::new();
    let plan = f.plan();
    let stream = f.source.join("resources/a.txt:unapproved");
    fs::write(&stream, b"unapproved stream data").unwrap();
    let report = apply_removal(approved(plan));
    assert_eq!(removed(&report), 0);
    assert!(!report.permanent_removal_attempted);
    assert!(report.retained_stage.is_none());
    assert_eq!(fs::read(stream).unwrap(), b"unapproved stream data");
    assert_eq!(
        fs::read(f.source.join("resources/a.txt")).unwrap(),
        b"original A"
    );
    f.sentinel();
}
#[test]
fn a_preexisting_child_read_lease_refuses_detachment_without_removing_data() {
    let f = Fixture::new();
    let plan = f.plan();
    let lease = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(f.source.join("resources/a.txt"))
        .unwrap();
    let report = apply_removal(approved(plan));
    assert!(!report.is_successful(), "{report:?}");
    assert!(!report.permanent_removal_attempted, "{report:?}");
    assert_eq!(removed(&report), 0);
    let failure = report.failure.as_ref().expect("native refusal");
    assert_eq!(failure.operation, "native held-object no-replace detach");
    assert!(
        matches!(
            failure.kind,
            RemovalFailureKind::Io {
                kind: io::ErrorKind::PermissionDenied,
                raw_os_error: Some(5)
            }
        ),
        "{report:?}"
    );
    assert!(report.retained_stage.is_none(), "{report:?}");
    assert_eq!(
        fs::read(f.source.join("resources/a.txt")).unwrap(),
        b"original A"
    );
    drop(lease);
    f.sentinel();
}
#[test]
fn a_shared_read_lease_never_turns_pending_deletion_into_false_complete_success() {
    let f = Fixture::new();
    let plan = f.plan();
    let mut lease = None;
    let report = run(&plan, &mut |phase, path| {
        if phase == Phase::AfterDetach {
            lease = Some(
                fs::OpenOptions::new()
                    .read(true)
                    .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                    .open(path.join("resources/a.txt"))?,
            );
        }
        Ok(())
    });
    assert!(
        lease.is_some(),
        "lease must be acquired after successful detachment: {report:?}"
    );
    if report.is_successful() {
        assert_eq!(
            fs::symlink_metadata(&f.source).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(report.retained_stage.is_none());
    } else {
        assert!(report.permanent_removal_attempted, "{report:?}");
        assert!(report.retained_stage.is_some(), "{report:?}");
        let operation = report.failure.as_ref().unwrap().operation;
        assert!(
            [
                "confirm removal after closing the object handle",
                "verify approved directory empty",
                "verify selected root empty",
                "dispose verified empty held root",
                "dispose approved held Windows object"
            ]
            .contains(&operation),
            "{report:?}"
        );
        assert!(
            report
                .outcomes
                .iter()
                .any(|row| row.state != RemovalEntryState::Removed)
        );
    }
    drop(lease);
    f.sentinel();
}
#[test]
fn stage_handle_blocks_external_replacement_before_cleanup() {
    let f = Fixture::new();
    let plan = f.plan();
    let destination = f.root.join("unapproved-stage-rename");
    let mut blocked = false;
    let report = run(&plan, &mut |phase, path| {
        if phase == Phase::BeforeCleanup {
            let error = fs::rename(path, &destination).unwrap_err();
            assert_eq!(error.raw_os_error(), Some(ERROR_SHARING_VIOLATION as i32));
            blocked = true;
        }
        Ok(())
    });
    assert!(blocked);
    assert!(report.is_successful(), "{report:?}");
    assert!(!destination.exists());
    f.sentinel();
}

#[test]
fn a_different_root_before_delete_open_is_refused_by_full_native_identity() {
    let f = Fixture::new();
    let plan = f.plan();
    let saved = f.root.join("saved-approved");
    let report = run(&plan, &mut |phase, _| {
        if phase == Phase::BeforeRootOpen {
            fs::rename(&f.source, &saved)?;
            fs::create_dir(&f.source)?;
            fs::write(f.source.join("unapproved"), b"replacement data")?;
        }
        Ok(())
    });
    assert_eq!(removed(&report), 0);
    assert!(!report.permanent_removal_attempted);
    assert!(report.retained_stage.is_none());
    assert_eq!(
        fs::read(f.source.join("unapproved")).unwrap(),
        b"replacement data"
    );
    assert_eq!(
        fs::read(saved.join("resources/a.txt")).unwrap(),
        b"original A"
    );
    f.sentinel();
}
#[test]
fn pinned_approved_file_refuses_a_new_writer_without_adopting_its_contents() {
    let f = Fixture::new();
    let plan = f.plan();
    let mut blocked = false;
    let report = run(&plan, &mut |phase, path| {
        if phase == Phase::BeforeUnlink && !blocked {
            let error = fs::write(path, b"unapproved overwrite").unwrap_err();
            assert_eq!(error.raw_os_error(), Some(ERROR_SHARING_VIOLATION as i32));
            blocked = true;
        }
        Ok(())
    });
    assert!(blocked);
    assert!(report.is_successful(), "{report:?}");
    f.sentinel();
}
#[test]
fn unapproved_enclosure_siblings_are_reported_and_never_deleted_as_cleanup() {
    let f = Fixture::new();
    let plan = f.plan();
    let report = run(&plan, &mut |phase, path| {
        if phase == Phase::BeforeCleanup {
            fs::write(path.join("unapproved-sibling"), b"unapproved data")?;
        }
        Ok(())
    });
    assert!(!report.is_successful());
    assert!(
        report
            .outcomes
            .iter()
            .all(|row| row.state == RemovalEntryState::Removed)
    );
    assert!(
        report
            .unrecognized_entries
            .iter()
            .any(|path| path.file_name() == Some(OsStr::new("unapproved-sibling")))
    );
    assert_eq!(
        fs::read(
            report
                .retained_stage
                .as_ref()
                .unwrap()
                .join("unapproved-sibling")
        )
        .unwrap(),
        b"unapproved data"
    );
    f.sentinel();
}
#[test]
fn two_native_removers_contend_for_one_delete_capability_without_double_removal() {
    use std::{sync::mpsc, time::Duration};
    let f = Fixture::new();
    let a = f.plan();
    let b = f.plan();
    let (ready, wait_ready) = mpsc::channel();
    let other_ready = ready.clone();
    let (go_a, wait_a) = mpsc::channel();
    let (go_b, wait_b) = mpsc::channel();
    let (a, b) = std::thread::scope(|scope| {
        let a = scope.spawn(move || {
            run(&a, &mut |phase, _| {
                if phase == Phase::BeforeRootOpen {
                    ready.send(()).map_err(io::Error::other)?;
                    wait_a
                        .recv_timeout(Duration::from_secs(10))
                        .map_err(io::Error::other)?;
                }
                Ok(())
            })
        });
        let b = scope.spawn(move || {
            run(&b, &mut |phase, _| {
                if phase == Phase::BeforeRootOpen {
                    other_ready.send(()).map_err(io::Error::other)?;
                    wait_b
                        .recv_timeout(Duration::from_secs(10))
                        .map_err(io::Error::other)?;
                }
                Ok(())
            })
        });
        wait_ready.recv_timeout(Duration::from_secs(10)).unwrap();
        wait_ready.recv_timeout(Duration::from_secs(10)).unwrap();
        go_a.send(()).unwrap();
        go_b.send(()).unwrap();
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_eq!(
        usize::from(a.is_successful()) + usize::from(b.is_successful()),
        1,
        "{a:?}\n{b:?}"
    );
    assert_eq!(removed(if a.is_successful() { &b } else { &a }), 0);
    assert_eq!(fs::read_dir(f.source.parent().unwrap()).unwrap().count(), 0);
    f.sentinel();
}
#[test]
fn interruption_child() {
    let Some(root) = std::env::var_os("ASK_WINDOWS_REMOVAL_TEST_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    assert_eq!(
        root.parent().unwrap(),
        fs::canonicalize(std::env::temp_dir()).unwrap()
    );
    assert!(
        root.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("ask-remove-windows-")
    );
    assert_eq!(
        fs::read(root.join("owned-interruption-root")).unwrap(),
        b"original owned Windows interruption fixture"
    );
    let phase = match std::env::var("ASK_WINDOWS_REMOVAL_TEST_PHASE")
        .unwrap()
        .as_str()
    {
        "before_detach" => Phase::BeforeDetach,
        "after_detach" => Phase::AfterDetach,
        "after_unlink" => Phase::AfterUnlink,
        _ => panic!("unsupported native child boundary"),
    };
    let home = root.join("home");
    let source = home.join(".agents/skills/sample");
    let f = std::mem::ManuallyDrop::new(Fixture { root, home, source });
    let plan = f.plan();
    let _report = run(&plan, &mut |observed, _| {
        if observed == phase {
            fs::write(f.root.join("ready"), b"native Windows boundary reached")?;
            loop {
                std::thread::park();
            }
        }
        Ok(())
    });
    panic!("child never reached selected native boundary");
}
#[test]
fn actual_process_kill_preserves_documented_original_full_or_partial_ntfs_payload() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    for phase in ["before_detach", "after_detach", "after_unlink"] {
        let f = Fixture::new();
        fs::write(
            f.root.join("owned-interruption-root"),
            b"original owned Windows interruption fixture",
        )
        .unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "removal::apply::windows::tests::interruption_child",
                "--nocapture",
            ])
            .env("ASK_WINDOWS_REMOVAL_TEST_ROOT", &f.root)
            .env("ASK_WINDOWS_REMOVAL_TEST_PHASE", phase)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !f.root.join("ready").is_file() {
            if child.try_wait().unwrap().is_some() {
                panic!("native child exited before {phase}");
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("native Windows boundary timed out: {phase}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());
        let stages = fs::read_dir(f.source.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with(".ask-remove-")
            })
            .collect::<Vec<_>>();
        assert_eq!(stages.len(), 1);
        let payload = stages[0].join("payload");
        if phase == "before_detach" {
            assert!(f.source.is_dir());
            assert!(!payload.exists());
            assert_eq!(
                fs::read(f.source.join("resources/a.txt")).unwrap(),
                b"original A"
            );
        } else {
            assert!(!f.source.exists());
            assert!(payload.join("SKILL.md").is_file());
            assert_eq!(
                fs::read(payload.join("resources/b.txt")).unwrap(),
                b"original B"
            );
            if phase == "after_detach" {
                assert_eq!(
                    fs::read(payload.join("resources/a.txt")).unwrap(),
                    b"original A"
                );
            } else {
                assert_eq!(
                    fs::symlink_metadata(payload.join("resources/a.txt"))
                        .unwrap_err()
                        .kind(),
                    io::ErrorKind::NotFound
                );
            }
        }
        f.sentinel();
    }
}
