use super::*;
use crate::{
    catalog::{Catalog, Scope},
    discovery::{DiscoveryLimits, list_skills},
    removal::{RemovalLimits, RemovalRequest, apply_removal, plan_removal},
    resolver::{InstallContext, native_platform},
};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
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
                "ask-remove-unix-{}-{}",
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
        fs::write(source.join("SKILL.md"),b"---\nname: sample\ndescription: Original inert permanent removal fixture.\n---\nNever execute.\n").unwrap();
        fs::write(source.join("resources/a.txt"), b"original A").unwrap();
        fs::write(source.join("resources/b.txt"), b"original B").unwrap();
        fs::write(
            root.join("outside-sentinel"),
            b"unrelated original sentinel",
        )
        .unwrap();
        Self { root, home, source }
    }
    fn plan(&self) -> RemovalPlan {
        let catalog = Catalog::bundled().unwrap();
        let mut context = InstallContext::new(native_platform().unwrap());
        context.user_home = Some(self.home.clone());
        let listing = list_skills(
            &catalog,
            "codex",
            Scope::User,
            &context,
            DiscoveryLimits::default(),
        )
        .unwrap();
        let location_index = listing
            .locations
            .iter()
            .position(|r| r.preferred_write)
            .unwrap();
        plan_removal(
            &catalog,
            RemovalRequest {
                listing,
                location_index,
                skill_name: "sample".into(),
                context,
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
            let m = fs::symlink_metadata(path).unwrap();
            if m.is_dir() {
                fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
                for e in fs::read_dir(path).unwrap() {
                    restore(&e.unwrap().path());
                }
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
        .filter(|row| row.state == RemovalEntryState::Removed)
        .count()
}
#[test]
fn actual_permanent_removal_is_complete_without_touching_siblings_or_leaving_a_stage() {
    let f = Fixture::new();
    let plan = f.plan();
    let total = plan.entries().len() + 1;
    let report = apply_removal(approved(plan));
    assert!(report.is_successful(), "{report:?}");
    assert_eq!(removed(&report), total);
    assert!(!f.source.exists());
    assert_eq!(fs::read_dir(f.source.parent().unwrap()).unwrap().count(), 0);
    f.sentinel();
}
#[test]
fn stale_contents_fail_read_only_before_stage_creation() {
    let f = Fixture::new();
    let plan = f.plan();
    fs::write(f.source.join("resources/a.txt"), b"edited before apply").unwrap();
    let report = apply_removal(approved(plan));
    assert!(!report.is_successful());
    assert_eq!(removed(&report), 0);
    assert!(report.retained_stage.is_none());
    assert_eq!(fs::read_dir(f.source.parent().unwrap()).unwrap().count(), 1);
    assert_eq!(
        fs::read(f.source.join("resources/a.txt")).unwrap(),
        b"edited before apply"
    );
    f.sentinel();
}
#[test]
fn last_moment_source_directory_substitution_is_restored_and_never_unlinked() {
    let f = Fixture::new();
    let plan = f.plan();
    let saved = f.root.join("saved-approved");
    let mut ran = false;
    let report = run(&plan, &mut |phase, _| {
        if phase == Phase::BeforeDetach {
            assert!(!ran);
            ran = true;
            fs::rename(&f.source, &saved)?;
            fs::create_dir(&f.source)?;
            fs::write(
                f.source.join("unapproved.txt"),
                b"unapproved replacement data",
            )?;
        }
        Ok(())
    });
    assert!(ran);
    assert!(!report.is_successful());
    assert_eq!(removed(&report), 0);
    assert!(report.restored_before_removal, "{report:?}");
    assert!(report.retained_stage.is_none());
    assert_eq!(
        fs::read(f.source.join("unapproved.txt")).unwrap(),
        b"unapproved replacement data"
    );
    assert_eq!(
        fs::read(saved.join("resources/a.txt")).unwrap(),
        b"original A"
    );
    f.sentinel();
}
#[test]
fn last_moment_source_link_is_restored_without_following_or_deleting_its_target() {
    let f = Fixture::new();
    let plan = f.plan();
    let saved = f.root.join("saved-approved");
    let outside = f.root.join("outside-directory");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"outside untouched").unwrap();
    let report = run(&plan, &mut |phase, _| {
        if phase == Phase::BeforeDetach {
            fs::rename(&f.source, &saved)?;
            symlink(&outside, &f.source)?;
        }
        Ok(())
    });
    assert_eq!(removed(&report), 0);
    assert!(report.restored_before_removal, "{report:?}");
    assert!(fs::symlink_metadata(&f.source).unwrap().is_symlink());
    assert_eq!(
        fs::read(outside.join("sentinel")).unwrap(),
        b"outside untouched"
    );
    assert!(saved.join("SKILL.md").is_file());
    f.sentinel();
}
#[test]
fn changed_detached_contents_are_restored_without_deleting_edits() {
    let f = Fixture::new();
    let plan = f.plan();
    let report = run(&plan, &mut |phase, path| {
        if phase == Phase::AfterDetach {
            fs::write(path.join("resources/a.txt"), b"edited after detach")?;
        }
        Ok(())
    });
    assert_eq!(removed(&report), 0);
    assert!(report.restored_before_removal, "{report:?}");
    assert_eq!(
        fs::read(f.source.join("resources/a.txt")).unwrap(),
        b"edited after detach"
    );
    f.sentinel();
}
#[test]
fn concurrent_original_name_occupant_survives_failed_no_replace_restoration() {
    let f = Fixture::new();
    let plan = f.plan();
    let report = run(&plan, &mut |phase, path| {
        if phase == Phase::AfterDetach {
            fs::write(path.join("resources/a.txt"), b"edited retained payload")?;
            fs::create_dir(&f.source)?;
            fs::write(f.source.join("winner"), b"concurrent winner")?;
        }
        Ok(())
    });
    assert!(!report.is_successful());
    assert_eq!(removed(&report), 0);
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
        b"edited retained payload"
    );
    f.sentinel();
}
#[test]
fn an_unknown_private_payload_does_not_gain_restoration_or_cleanup_authority() {
    let f = Fixture::new();
    let plan = f.plan();
    let report = run(&plan, &mut |phase, path| {
        if phase == Phase::AfterStage {
            fs::create_dir(path.join("payload"))?;
            fs::write(
                path.join("payload/unapproved"),
                b"unapproved staging occupant",
            )?;
        }
        Ok(())
    });
    assert_eq!(removed(&report), 0);
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
        b"unapproved staging occupant"
    );
    f.sentinel();
}
#[test]
fn a_new_private_entry_before_unlink_stops_without_removing_unknown_data() {
    let f = Fixture::new();
    let plan = f.plan();
    let mut ran = false;
    let report = run(&plan, &mut |phase, path| {
        if phase == Phase::BeforeUnlink && !ran {
            ran = true;
            fs::write(path.parent().unwrap().join("new-unapproved"), b"new data")?;
        }
        Ok(())
    });
    assert!(ran);
    assert_eq!(removed(&report), 0);
    assert!(report.retained_stage.is_some());
    assert!(!report.is_successful());
    f.sentinel();
}
#[test]
fn injected_failure_after_first_native_unlink_preserves_removed_and_not_attempted_outcomes() {
    let f = Fixture::new();
    let plan = f.plan();
    let mut ran = false;
    let report = run(&plan, &mut |phase, _| {
        if phase == Phase::AfterUnlink && !ran {
            ran = true;
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "controlled post-unlink failure",
            ));
        }
        Ok(())
    });
    assert_eq!(removed(&report), 1);
    assert!(
        report
            .outcomes
            .iter()
            .any(|r| r.state == RemovalEntryState::NotAttempted)
    );
    assert!(report.retained_stage.is_some());
    assert!(!report.restored_before_removal);
    assert!(!report.is_successful());
    f.sentinel();
}
#[test]
fn native_directory_permission_denial_retains_partial_payload_without_chmod() {
    let f = Fixture::new();
    fs::create_dir(f.source.join("aaa")).unwrap();
    fs::write(f.source.join("aaa/first"), b"approved first data").unwrap();
    fs::create_dir(f.source.join("zzz")).unwrap();
    fs::write(f.source.join("zzz/denied"), b"approved denied data").unwrap();
    fs::set_permissions(f.source.join("zzz"), fs::Permissions::from_mode(0o555)).unwrap();
    let plan = f.plan();
    let report = apply_removal(approved(plan));
    assert!(
        !report.is_successful(),
        "unprivileged native account required"
    );
    assert!(removed(&report) > 0);
    assert!(
        matches!(
            &report.failure.as_ref().unwrap().kind,
            RemovalFailureKind::Io {
                kind: io::ErrorKind::PermissionDenied,
                ..
            }
        ),
        "{report:?}"
    );
    assert!(
        report
            .outcomes
            .iter()
            .any(|row| row.state == RemovalEntryState::Remaining)
    );
    let remaining = report.retained_stage.as_ref().unwrap().join("payload/zzz");
    assert_eq!(
        fs::metadata(&remaining).unwrap().permissions().mode() & 0o7777,
        0o555
    );
    assert_eq!(
        fs::read(remaining.join("denied")).unwrap(),
        b"approved denied data"
    );
    f.sentinel();
}
#[test]
fn replacing_the_empty_stage_before_cleanup_never_deletes_the_replacement() {
    let f = Fixture::new();
    let plan = f.plan();
    let moved = f.root.join("relocated-owned-enclosure");
    let mut replaced = None;
    let report = run(&plan, &mut |phase, path| {
        if phase == Phase::BeforeCleanup {
            fs::rename(path, &moved)?;
            fs::create_dir(path)?;
            fs::write(path.join("unrelated"), b"replacement sentinel")?;
            replaced = Some(path.to_owned());
        }
        Ok(())
    });
    assert!(
        report
            .outcomes
            .iter()
            .all(|r| r.state == RemovalEntryState::Removed)
    );
    assert!(!report.is_successful());
    assert!(!report.cleanup_failures.is_empty());
    assert_eq!(
        fs::read(replaced.unwrap().join("unrelated")).unwrap(),
        b"replacement sentinel"
    );
    assert!(moved.is_dir());
    assert!(report.stage_location_uncertain);
    f.sentinel();
}

#[test]
fn native_parent_permission_denial_never_detaches_or_creates_an_enclosure() {
    let f = Fixture::new();
    fs::set_permissions(
        f.source.parent().unwrap(),
        fs::Permissions::from_mode(0o555),
    )
    .unwrap();
    let report = apply_removal(approved(f.plan()));
    assert!(
        !report.is_successful(),
        "unprivileged native test account required"
    );
    assert_eq!(removed(&report), 0);
    assert!(report.retained_stage.is_none());
    assert!(
        matches!(
            &report.failure.as_ref().unwrap().kind,
            RemovalFailureKind::Io {
                kind: io::ErrorKind::PermissionDenied,
                ..
            }
        ),
        "{report:?}"
    );
    assert_eq!(
        fs::read(f.source.join("resources/a.txt")).unwrap(),
        b"original A"
    );
    assert_eq!(fs::read_dir(f.source.parent().unwrap()).unwrap().count(), 1);
    f.sentinel();
}

#[test]
fn interruption_child() {
    let Some(root) = std::env::var_os("ASK_REMOVAL_TEST_ROOT") else {
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
            .starts_with("ask-remove-unix-")
    );
    assert_eq!(
        fs::read(root.join("owned-interruption-root")).unwrap(),
        b"original test-owned interruption fixture"
    );
    let phase = match std::env::var("ASK_REMOVAL_TEST_PHASE").unwrap().as_str() {
        "before_detach" => Phase::BeforeDetach,
        "after_detach" => Phase::AfterDetach,
        "after_unlink" => Phase::AfterUnlink,
        _ => panic!("unsupported child phase"),
    };
    let home = root.join("home");
    let source = home.join(".agents/skills/sample");
    let f = std::mem::ManuallyDrop::new(Fixture { root, home, source });
    let plan = f.plan();
    let _report = run(&plan, &mut |observed, _| {
        if observed == phase {
            fs::write(f.root.join("ready"), b"native boundary reached")?;
            loop {
                std::thread::park();
            }
        }
        Ok(())
    });
    panic!("child never reached the selected native interruption boundary");
}
#[test]
fn actual_process_kill_retains_only_the_documented_full_or_partial_payload() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    for phase in ["before_detach", "after_detach", "after_unlink"] {
        let f = Fixture::new();
        fs::write(
            f.root.join("owned-interruption-root"),
            b"original test-owned interruption fixture",
        )
        .unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "removal::apply::unix::tests::interruption_child",
                "--nocapture",
            ])
            .env("ASK_REMOVAL_TEST_ROOT", &f.root)
            .env("ASK_REMOVAL_TEST_PHASE", phase)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while !f.root.join("ready").is_file() {
            if child.try_wait().unwrap().is_some() {
                panic!("native interruption child exited before {phase}");
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("native interruption boundary timed out: {phase}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());
        let stages = fs::read_dir(f.source.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
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
                assert!(!payload.join("resources/a.txt").exists());
            }
        }
        f.sentinel();
        // Only this original fixture owner tears down its own complete root.
        // Production code never infers orphan ownership from a stage prefix.
    }
}

#[test]
fn explicit_caller_ancestor_alias_resolves_to_the_same_approved_physical_payload() {
    let f = Fixture::new();
    let alias = f.root.join("explicit-parent-alias");
    symlink(&f.root, &alias).unwrap();
    let selected = std::mem::ManuallyDrop::new(Fixture {
        root: f.root.clone(),
        home: alias.join("home"),
        source: f.source.clone(),
    });
    let plan = selected.plan();
    assert_eq!(plan.path(), f.source);
    let report = apply_removal(approved(plan));
    assert!(report.is_successful(), "{report:?}");
    assert!(!f.source.exists());
    assert!(f.home.is_dir());
    f.sentinel();
}
#[test]
fn a_new_hardlink_after_approval_fails_preflight_without_removing_either_name() {
    let f = Fixture::new();
    let plan = f.plan();
    let alias = f.root.join("outside-hardlink");
    fs::hard_link(f.source.join("resources/a.txt"), &alias).unwrap();
    let report = apply_removal(approved(plan));
    assert_eq!(removed(&report), 0);
    assert!(report.retained_stage.is_none());
    assert_eq!(fs::read(&alias).unwrap(), b"original A");
    assert_eq!(
        fs::read(f.source.join("resources/a.txt")).unwrap(),
        b"original A"
    );
    f.sentinel();
}
#[test]
fn observable_container_link_replacement_is_rejected_before_any_stage_or_unlink() {
    let f = Fixture::new();
    let plan = f.plan();
    let container = f.source.parent().unwrap();
    let saved = f.root.join("saved-container");
    let outside = f.root.join("outside-container");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"outside container data").unwrap();
    let report = run(&plan, &mut |phase, _| {
        if phase == Phase::BeforeStage {
            fs::rename(container, &saved)?;
            symlink(&outside, container)?;
        }
        Ok(())
    });
    assert_eq!(removed(&report), 0);
    assert!(report.retained_stage.is_none());
    assert_eq!(
        fs::read(outside.join("sentinel")).unwrap(),
        b"outside container data"
    );
    assert_eq!(
        fs::read(saved.join("sample/resources/a.txt")).unwrap(),
        b"original A"
    );
    f.sentinel();
}

#[test]
fn two_verified_removals_race_for_one_root_without_double_deletion_or_stage_loss() {
    use std::{sync::mpsc, time::Duration};
    let f = Fixture::new();
    let first = f.plan();
    let second = f.plan();
    let (ready_tx, ready_rx) = mpsc::channel();
    let other_ready = ready_tx.clone();
    let (go_first, wait_first) = mpsc::channel();
    let (go_second, wait_second) = mpsc::channel();
    let (a, b) = std::thread::scope(|scope| {
        let a = scope.spawn(move || {
            run(&first, &mut |phase, _| {
                if phase == Phase::BeforeDetach {
                    ready_tx.send(()).map_err(io::Error::other)?;
                    wait_first
                        .recv_timeout(Duration::from_secs(10))
                        .map_err(io::Error::other)?;
                }
                Ok(())
            })
        });
        let b = scope.spawn(move || {
            run(&second, &mut |phase, _| {
                if phase == Phase::BeforeDetach {
                    other_ready.send(()).map_err(io::Error::other)?;
                    wait_second
                        .recv_timeout(Duration::from_secs(10))
                        .map_err(io::Error::other)?;
                }
                Ok(())
            })
        });
        ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        go_first.send(()).unwrap();
        go_second.send(()).unwrap();
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_eq!(
        usize::from(a.is_successful()) + usize::from(b.is_successful()),
        1,
        "{a:?}\n{b:?}"
    );
    let loser = if a.is_successful() { &b } else { &a };
    assert_eq!(removed(loser), 0);
    assert!(!f.source.exists());
    assert_eq!(fs::read_dir(f.source.parent().unwrap()).unwrap().count(), 0);
    f.sentinel();
}

#[test]
fn changed_protected_context_ancestry_is_rechecked_before_any_write() {
    let f = Fixture::new();
    let outside = f.root.join("protected-context");
    fs::create_dir_all(outside.join("workspace")).unwrap();
    fs::create_dir(f.source.join("resources/workspace")).unwrap();
    let alias = f.root.join("context-parent-alias");
    symlink(&outside, &alias).unwrap();
    let mut context = InstallContext::new(native_platform().unwrap());
    context.user_home = Some(f.home.clone());
    context.application_workspace = Some(alias.join("workspace"));
    let catalog = Catalog::bundled().unwrap();
    let listing = list_skills(
        &catalog,
        "codex",
        Scope::User,
        &context,
        DiscoveryLimits::default(),
    )
    .unwrap();
    let location_index = listing
        .locations
        .iter()
        .position(|row| row.preferred_write)
        .unwrap();
    let plan = plan_removal(
        &catalog,
        RemovalRequest {
            listing,
            location_index,
            skill_name: "sample".into(),
            context,
            limits: RemovalLimits::default(),
        },
    )
    .unwrap();
    let approval = approved(plan);
    fs::remove_file(&alias).unwrap();
    symlink(f.source.join("resources"), &alias).unwrap();
    let report = apply_removal(approval);
    assert_eq!(removed(&report), 0);
    assert!(report.retained_stage.is_none());
    assert!(
        matches!(
            report.failure.as_ref().map(|failure| &failure.kind),
            Some(RemovalFailureKind::Preflight(RemovalError::ProtectedRoot(
                _
            )))
        ),
        "{report:?}"
    );
    assert_eq!(
        fs::read(f.source.join("resources/a.txt")).unwrap(),
        b"original A"
    );
    assert_eq!(fs::read_dir(f.source.parent().unwrap()).unwrap().count(), 1);
    f.sentinel();
}
