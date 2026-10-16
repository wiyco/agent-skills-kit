use super::*;
use crate::{
    catalog::{Catalog, Scope},
    planner::{InstallRequest, PlanLimits, plan_install},
    resolver::{InstallContext, native_platform},
    validation::{InventoryLimits, inventory_skill},
};
use std::os::unix::fs::symlink;

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let root = std::env::temp_dir().join(format!(
                "ask-copy-race-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&root) {
                Ok(()) => return Self(root),
                Err(observed) if observed.kind() == io::ErrorKind::AlreadyExists => (),
                Err(observed) => panic!("owned fixture: {observed}"),
            }
        }
        panic!("fixture collisions exhausted")
    }
    fn plan(&self, targets: &[&str]) -> InstallPlan {
        let source = self.0.join("source/sample");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("SKILL.md"),
            b"---\nname: sample\ndescription: inert fault fixture\n---\n",
        )
        .unwrap();
        std::fs::write(source.join("resource"), b"original").unwrap();
        let home = self.0.join("home");
        std::fs::create_dir_all(home.join(".agents/skills")).unwrap();
        let mut context = InstallContext::new(native_platform().unwrap());
        context.user_home = Some(home);
        let sources = [inventory_skill(&source, InventoryLimits::default()).unwrap()];
        plan_install(
            &Catalog::bundled().unwrap(),
            InstallRequest {
                skills: &sources,
                target_ids: targets,
                scope: Scope::User,
                context: &context,
                limits: PlanLimits::default(),
            },
        )
        .unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("owned fixture teardown");
    }
}
fn run(
    plan: &InstallPlan,
    options: ApplyOptions,
    hook: &mut impl FnMut(Phase, &Path) -> io::Result<()>,
) -> InstallReport {
    verify_plan(plan, options.verification).unwrap();
    let mut report = InstallReport {
        preflight_error: None,
        cleanup_errors: Vec::new(),
        outcomes: plan
            .destinations()
            .iter()
            .map(|destination| DestinationOutcome {
                path: destination.path().map(Path::to_owned),
                attributions: Vec::new(),
                status: OutcomeStatus::NotAttempted,
            })
            .collect(),
    };
    apply_with_hook(plan, options, &mut report, hook);
    report
}
fn no_stages(parent: &Path) {
    assert!(std::fs::read_dir(parent).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".ask-stage-")
    }));
}

#[test]
fn native_exclusive_commit_preserves_a_last_moment_winner() {
    for kind in ["file", "empty-directory", "link"] {
        let root = Root::new();
        let plan = root.plan(&["codex"]);
        let destination = plan.destinations()[0].path().unwrap();
        let outside = root.0.join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("sentinel"), b"unrelated").unwrap();
        let report = run(&plan, ApplyOptions::default(), &mut |phase, path| {
            if phase == Phase::BeforeCommit {
                match kind {
                    "file" => std::fs::write(path, b"concurrent winner")?,
                    "empty-directory" => std::fs::create_dir(path)?,
                    "link" => symlink(&outside, path)?,
                    _ => unreachable!(),
                }
            }
            Ok(())
        });
        assert!(
            matches!(report.outcomes[0].status, OutcomeStatus::Failed(_)),
            "{report:#?}"
        );
        assert!(report.cleanup_errors.is_empty(), "{report:#?}");
        match kind {
            "file" => assert_eq!(std::fs::read(destination).unwrap(), b"concurrent winner"),
            "empty-directory" => assert_eq!(std::fs::read_dir(destination).unwrap().count(), 0),
            "link" => assert_eq!(std::fs::read_link(destination).unwrap(), outside),
            _ => unreachable!(),
        }
        assert_eq!(
            std::fs::read(root.0.join("outside/sentinel")).unwrap(),
            b"unrelated"
        );
        no_stages(destination.parent().unwrap());
    }
}

#[test]
fn injected_copy_permission_commit_and_interruption_failures_clean_only_owned_stage() {
    for (phase, code) in [
        (Phase::BeforeCopyFile, libc::EIO),
        (Phase::BeforeCopyFile, libc::EACCES),
        (Phase::BeforeCommit, libc::ENOSYS),
        (Phase::AfterStage, libc::EINTR),
    ] {
        let root = Root::new();
        let plan = root.plan(&["codex"]);
        let mut fired = false;
        let report = run(&plan, ApplyOptions::default(), &mut |current, _| {
            if current == phase && !fired {
                fired = true;
                Err(io::Error::from_raw_os_error(code))
            } else {
                Ok(())
            }
        });
        assert!(fired);
        assert!(matches!(
            report.outcomes[0].status,
            OutcomeStatus::Failed(_)
        ));
        assert!(report.cleanup_errors.is_empty(), "{report:#?}");
        let destination = plan.destinations()[0].path().unwrap();
        assert!(!destination.exists());
        no_stages(destination.parent().unwrap());
    }
}

#[test]
fn source_edits_during_copy_or_after_staging_invalidate_commit() {
    for phase in [
        Phase::BeforeCopyFile,
        Phase::AfterSourceRead,
        Phase::AfterStage,
    ] {
        let root = Root::new();
        let plan = root.plan(&["codex"]);
        let resource = root.0.join("source/sample/resource");
        let mut fired = false;
        let report = run(&plan, ApplyOptions::default(), &mut |current, path| {
            if current == phase
                && !fired
                && (phase == Phase::AfterStage || path.ends_with("resource"))
            {
                fired = true;
                std::fs::write(&resource, b"changed")?;
            }
            Ok(())
        });
        assert!(fired);
        assert!(
            matches!(report.outcomes[0].status, OutcomeStatus::Failed(_)),
            "{report:#?}"
        );
        assert!(report.cleanup_errors.is_empty(), "{report:#?}");
        let destination = plan.destinations()[0].path().unwrap();
        assert!(!destination.exists());
        no_stages(destination.parent().unwrap());
    }
}

#[test]
fn cleanup_substitution_leaves_unrelated_stage_and_reports_uncertainty() {
    let root = Root::new();
    let plan = root.plan(&["codex"]);
    let mut saved = None;
    let report = run(&plan, ApplyOptions::default(), &mut |phase, path| {
        if phase == Phase::AfterStage {
            return Err(io::Error::from_raw_os_error(libc::EIO));
        }
        if phase == Phase::BeforeCleanup {
            std::fs::rename(path, root.0.join("moved-owned-stage"))?;
            std::fs::create_dir(path)?;
            std::fs::write(path.join("sentinel"), b"unrelated")?;
            saved = Some(path.to_owned());
        }
        Ok(())
    });
    assert!(matches!(
        report.outcomes[0].status,
        OutcomeStatus::Failed(_)
    ));
    assert!(
        report
            .cleanup_errors
            .iter()
            .any(|failure| failure.kind == ApplyErrorKind::StageChanged)
    );
    assert_eq!(
        std::fs::read(saved.unwrap().join("sentinel")).unwrap(),
        b"unrelated"
    );
    assert!(root.0.join("moved-owned-stage/sample/SKILL.md").is_file());
}

#[test]
fn batch_reports_partial_success_and_not_attempted_without_rollback() {
    for stop in [true, false] {
        let root = Root::new();
        let plan = root.plan(&["claude-code", "codex", "opencode"]);
        let mut count = 0;
        let report = run(
            &plan,
            ApplyOptions {
                stop_on_failure: stop,
                ..ApplyOptions::default()
            },
            &mut |phase, _| {
                if phase == Phase::BeforeDestination {
                    count += 1;
                    if count == 2 {
                        return Err(io::Error::from_raw_os_error(libc::EACCES));
                    }
                }
                Ok(())
            },
        );
        assert_eq!(report.outcomes[0].status, OutcomeStatus::Created);
        assert!(matches!(
            report.outcomes[1].status,
            OutcomeStatus::Failed(_)
        ));
        assert_eq!(
            report.outcomes[2].status,
            if stop {
                OutcomeStatus::NotAttempted
            } else {
                OutcomeStatus::Created
            }
        );
        assert!(
            report.outcomes[0]
                .path
                .as_ref()
                .unwrap()
                .join("SKILL.md")
                .is_file()
        );
        assert!(report.cleanup_errors.is_empty(), "{report:#?}");
    }
}

#[test]
fn concurrent_native_commits_have_one_winner() {
    let root = Root::new();
    let plan = Arc::new(root.plan(&["codex"]));
    let (anchor, _) = plan.destinations()[0]
        .namespace()
        .unwrap()
        .retained_anchor();
    let stats = rustix::fs::fstatfs(anchor).unwrap();
    // Direct stdout intentionally survives libtest capture: preserve actual
    // runner filesystem evidence for the commit-race test in required CI logs.
    use std::io::Write;
    writeln!(
        std::io::stdout(),
        "native copy QA: os={} arch={} filesystem_type={:#x}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        stats.f_type
    )
    .unwrap();
    let (ready, arrivals) = std::sync::mpsc::channel();
    let mut gates = Vec::new();
    let handles = (0..2)
        .map(|_| {
            let plan = Arc::clone(&plan);
            let ready = ready.clone();
            let (release, gate) = std::sync::mpsc::channel();
            gates.push(release);
            std::thread::spawn(move || {
                run(&plan, ApplyOptions::default(), &mut |phase, _| {
                    if phase == Phase::BeforeCommit {
                        ready
                            .send(())
                            .map_err(|_| io::Error::other("race supervisor closed"))?;
                        gate.recv_timeout(std::time::Duration::from_secs(5))
                            .map_err(|_| io::Error::other("race gate timeout"))?;
                    }
                    Ok(())
                })
            })
        })
        .collect::<Vec<_>>();
    for _ in 0..2 {
        arrivals
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("both installers reach commit");
    }
    for gate in gates {
        gate.send(()).unwrap();
    }
    let reports = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        reports
            .iter()
            .filter(|report| report.is_successful())
            .count(),
        1,
        "{reports:#?}"
    );
    assert_eq!(
        reports
            .iter()
            .filter(|report| matches!(report.outcomes[0].status, OutcomeStatus::Created))
            .count(),
        1
    );
    assert!(
        reports
            .iter()
            .all(|report| report.cleanup_errors.is_empty()),
        "{reports:#?}"
    );
    assert_eq!(
        std::fs::read(plan.destinations()[0].path().unwrap().join("resource")).unwrap(),
        b"original"
    );
    no_stages(plan.destinations()[0].path().unwrap().parent().unwrap());
}

#[test]
fn failed_new_parent_batch_retains_only_reported_ancestors() {
    let root = Root::new();
    let plan = root.plan(&["claude-code"]);
    let report = run(&plan, ApplyOptions::default(), &mut |phase, _| {
        if phase == Phase::AfterStage {
            Err(io::Error::from_raw_os_error(libc::EIO))
        } else {
            Ok(())
        }
    });
    assert!(matches!(
        report.outcomes[0].status,
        OutcomeStatus::Failed(_)
    ));
    assert_eq!(
        report
            .cleanup_errors
            .iter()
            .filter(|failure| failure.kind == ApplyErrorKind::CreatedParentRetained)
            .count(),
        2
    );
    let parent = plan.destinations()[0].path().unwrap().parent().unwrap();
    assert!(parent.is_dir());
    assert_eq!(std::fs::read_dir(parent).unwrap().count(), 0);
    assert!(!plan.destinations()[0].path().unwrap().exists());
}

#[test]
fn post_commit_cleanup_failure_does_not_hide_a_successful_install() {
    let root = Root::new();
    let plan = root.plan(&["codex"]);
    let mut stage = None;
    let report = run(&plan, ApplyOptions::default(), &mut |phase, path| {
        if phase == Phase::BeforeCleanup {
            stage = Some(path.to_owned());
            Err(io::Error::from_raw_os_error(libc::EACCES))
        } else {
            Ok(())
        }
    });
    assert_eq!(report.outcomes[0].status, OutcomeStatus::Created);
    assert!(!report.is_successful());
    assert!(!report.cleanup_errors.is_empty());
    assert!(stage.unwrap().is_dir());
    assert!(
        plan.destinations()[0]
            .path()
            .unwrap()
            .join("SKILL.md")
            .is_file()
    );
}

#[test]
fn parent_replaced_after_staging_is_rejected_without_following_its_link() {
    let root = Root::new();
    let plan = root.plan(&["codex"]);
    let outside = root.0.join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("sentinel"), b"untouched").unwrap();
    let report = run(&plan, ApplyOptions::default(), &mut |phase, _| {
        if phase == Phase::AfterStage {
            let selected = root.0.join("home/.agents");
            std::fs::rename(&selected, root.0.join("moved-parent"))?;
            symlink(&outside, &selected)?;
        }
        Ok(())
    });
    assert!(matches!(
        report.outcomes[0].status,
        OutcomeStatus::Failed(_)
    ));
    assert_eq!(
        std::fs::read(outside.join("sentinel")).unwrap(),
        b"untouched"
    );
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
    assert!(!root.0.join("moved-parent/skills/sample").exists());
}

#[test]
fn stage_edits_after_byte_verification_invalidate_commit() {
    let root = Root::new();
    let plan = root.plan(&["codex"]);
    let report = run(&plan, ApplyOptions::default(), &mut |phase, path| {
        if phase == Phase::AfterStage {
            std::fs::write(path.join("sample/resource"), b"tampered")?;
        }
        Ok(())
    });
    assert!(
        matches!(
            report.outcomes[0].status,
            OutcomeStatus::Failed(ApplyError {
                kind: ApplyErrorKind::StageChanged,
                ..
            })
        ),
        "{report:#?}"
    );
    assert!(report.cleanup_errors.is_empty(), "{report:#?}");
    let destination = plan.destinations()[0].path().unwrap();
    assert!(!destination.exists());
    no_stages(destination.parent().unwrap());
}

#[test]
fn substituted_copy_inputs_are_rejected_before_reading_or_blocking() {
    for kind in ["link", "directory", "hardlink", "fifo"] {
        let root = Root::new();
        let plan = Arc::new(root.plan(&["codex"]));
        let source = root.0.join("source/sample/resource");
        let sentinel = root.0.join("outside");
        std::fs::write(&sentinel, b"untouched").unwrap();
        let worker_plan = Arc::clone(&plan);
        let outside = sentinel.clone();
        let (done, finished) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut changed = false;
            let report = run(&worker_plan, ApplyOptions::default(), &mut |phase, path| {
                if phase == Phase::BeforeCopyFile && path.ends_with("resource") && !changed {
                    changed = true;
                    std::fs::remove_file(&source)?;
                    match kind {
                        "link" => symlink(&outside, &source)?,
                        "directory" => std::fs::create_dir(&source)?,
                        "hardlink" => std::fs::hard_link(&outside, &source)?,
                        "fifo" => {
                            let status = std::process::Command::new("/usr/bin/mkfifo")
                                .arg(&source)
                                .status()?;
                            if !status.success() {
                                return Err(io::Error::other("owned FIFO fixture creation failed"));
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                Ok(())
            });
            done.send(report).unwrap();
        });
        let report = finished
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("unsafe input must not block open/read");
        worker.join().unwrap();
        assert!(
            matches!(report.outcomes[0].status, OutcomeStatus::Failed(_)),
            "{kind}: {report:#?}"
        );
        assert!(report.cleanup_errors.is_empty(), "{report:#?}");
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"untouched");
        assert!(!plan.destinations()[0].path().unwrap().exists());
        no_stages(plan.destinations()[0].path().unwrap().parent().unwrap());
    }
}

#[test]
fn failure_after_a_partial_file_write_removes_only_owned_staging() {
    let root = Root::new();
    let initial = root.plan(&["codex"]);
    drop(initial);
    let source = root.0.join("source/sample");
    let bytes = vec![0x5a; 32768];
    std::fs::write(source.join("resource"), &bytes).unwrap();
    let sources = [inventory_skill(&source, InventoryLimits::default()).unwrap()];
    let mut context = InstallContext::new(native_platform().unwrap());
    context.user_home = Some(root.0.join("home"));
    let plan = plan_install(
        &Catalog::bundled().unwrap(),
        InstallRequest {
            skills: &sources,
            target_ids: &["codex"],
            scope: Scope::User,
            context: &context,
            limits: PlanLimits::default(),
        },
    )
    .unwrap();
    let sentinel = root.0.join("home/.agents/skills/unrelated");
    std::fs::write(&sentinel, b"preserved").unwrap();
    let mut partial = false;
    let report = run(&plan, ApplyOptions::default(), &mut |phase, path| {
        if phase == Phase::AfterWriteChunk && path.ends_with("resource") {
            let length = std::fs::metadata(path)?.len();
            assert!(length > 0 && length < bytes.len() as u64);
            partial = true;
            return Err(io::Error::from_raw_os_error(libc::EIO));
        }
        Ok(())
    });
    assert!(partial);
    assert!(matches!(
        report.outcomes[0].status,
        OutcomeStatus::Failed(_)
    ));
    assert!(report.cleanup_errors.is_empty(), "{report:#?}");
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"preserved");
    assert_eq!(std::fs::read(source.join("resource")).unwrap(), bytes);
    assert!(!plan.destinations()[0].path().unwrap().exists());
    no_stages(plan.destinations()[0].path().unwrap().parent().unwrap());
}
