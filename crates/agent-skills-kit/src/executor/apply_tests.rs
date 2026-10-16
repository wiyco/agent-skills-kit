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
        Self::new_in(&std::env::temp_dir())
    }
    fn new_in(base: &Path) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let root = base.join(format!(
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
        self.existing_plan(targets)
    }
    fn existing_plan(&self, targets: &[&str]) -> InstallPlan {
        let source = self.0.join("source/sample");
        let home = self.0.join("home");
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
    let mut report = super::super::initial_report(plan);
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
    let saved = saved.unwrap();
    assert_eq!(std::fs::read(saved.join("sentinel")).unwrap(), b"unrelated");
    assert!(root.0.join("moved-owned-stage/sample/SKILL.md").is_file());
    use crate::receipt::{OperationId, Receipt, ReceiptFailureCode, ReceiptLimits};
    let receipt = Receipt::from_report(
        &plan,
        &report,
        OperationId::new("cleanup-uncertain").unwrap(),
        ReceiptLimits::default(),
    )
    .unwrap();
    let parsed = Receipt::from_json(
        &receipt.to_json(ReceiptLimits::default()).unwrap(),
        ReceiptLimits::default(),
    )
    .unwrap();
    assert!(
        parsed
            .document()
            .cleanup_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == ReceiptFailureCode::StageChanged)
    );
    let retry = root.plan(&["codex"]);
    let retry_report = apply_plan(&retry, ApplyOptions::default());
    assert!(retry_report.is_successful(), "{retry_report:#?}");
    assert_eq!(std::fs::read(saved.join("sentinel")).unwrap(), b"unrelated");
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
        use crate::receipt::{OperationId, Receipt, ReceiptLimits, ReceiptStatus};
        use std::os::unix::fs::MetadataExt;
        let recorded = Receipt::from_report(
            &plan,
            &report,
            OperationId::new("partial-batch").unwrap(),
            ReceiptLimits::default(),
        )
        .unwrap();
        let parsed = Receipt::from_json(
            &recorded.to_json(ReceiptLimits::default()).unwrap(),
            ReceiptLimits::default(),
        )
        .unwrap();
        assert_eq!(
            parsed.document().destinations[0].status,
            ReceiptStatus::Created
        );
        assert_eq!(
            parsed.document().destinations[1].status,
            ReceiptStatus::Failed
        );
        assert_eq!(
            parsed.document().destinations[2].status,
            if stop {
                ReceiptStatus::NotAttempted
            } else {
                ReceiptStatus::Created
            }
        );
        let first = report.outcomes[0].path.as_ref().unwrap();
        let inode = std::fs::metadata(first).unwrap().ino();
        let retry = root.plan(&["claude-code", "codex", "opencode"]);
        let retry_report = apply_plan(&retry, ApplyOptions::default());
        assert!(retry_report.is_successful(), "{retry_report:#?}");
        assert_eq!(retry_report.outcomes[0].status, OutcomeStatus::Unchanged);
        assert_eq!(std::fs::metadata(first).unwrap().ino(), inode);
        let retried = Receipt::from_report(
            &retry,
            &retry_report,
            OperationId::new("partial-retry").unwrap(),
            ReceiptLimits::default(),
        )
        .unwrap();
        assert_eq!(
            retried.document().destinations[0].status,
            ReceiptStatus::Unchanged
        );
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
    use crate::receipt::{OperationId, Receipt, ReceiptFailureCode, ReceiptLimits};
    let receipt = Receipt::from_report(
        &plan,
        &report,
        OperationId::new("retained-parents").unwrap(),
        ReceiptLimits::default(),
    )
    .unwrap();
    assert_eq!(
        receipt
            .document()
            .cleanup_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == ReceiptFailureCode::CreatedParentRetained)
            .count(),
        2
    );
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

#[test]
fn changed_private_parent_or_stage_modes_block_commit_and_cleanup() {
    use std::os::unix::fs::PermissionsExt;
    for stage_changed in [false, true] {
        let root = Root::new();
        let plan = root.plan(&["claude-code"]);
        let mut retained = None;
        let report = run(&plan, ApplyOptions::default(), &mut |phase, path| {
            if phase == Phase::AfterStage {
                let changed = if stage_changed {
                    path
                } else {
                    path.parent().unwrap()
                };
                std::fs::set_permissions(changed, std::fs::Permissions::from_mode(0o777))?;
                retained = Some(path.to_owned());
            }
            Ok(())
        });
        assert!(
            matches!(report.outcomes[0].status, OutcomeStatus::Failed(_)),
            "{report:#?}"
        );
        assert!(!report.cleanup_errors.is_empty());
        assert!(!plan.destinations()[0].path().unwrap().exists());
        assert!(
            retained.unwrap().join("sample/SKILL.md").is_file(),
            "uncertain private context must be retained"
        );
    }
}

/// Run only this trusted test executable as a child; skill contents remain inert.
#[test]
fn process_kill_at_commit_boundary_preserves_payload_and_unowned_remnants() {
    use std::{
        io::{BufRead, Write},
        mem::ManuallyDrop,
        os::unix::{fs::MetadataExt, process::ExitStatusExt},
        process::{Child, Command, Stdio},
        sync::mpsc,
        time::Duration,
    };
    const CHILD_ROOT: &str = "ASK_TEST_KILL_OWNED_ROOT";
    const CHILD_PHASE: &str = "ASK_TEST_KILL_PHASE";
    const READY: &str = "ASK_TEST_KILL_READY";
    if let Some(path) = std::env::var_os(CHILD_ROOT) {
        // The parent owns teardown. Child-only variables never mutate this process.
        let root = ManuallyDrop::new(Root(PathBuf::from(path)));
        assert!(root.0.starts_with(std::env::temp_dir()));
        assert!(
            root.0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("ask-copy-race-")
        );
        let phase = match std::env::var(CHILD_PHASE).unwrap().as_str() {
            "before" => Phase::BeforeCommit,
            "after" => Phase::BeforeCleanup,
            _ => panic!("unknown supervised phase"),
        };
        let plan = root.existing_plan(&["codex"]);
        let _ = run(&plan, ApplyOptions::default(), &mut |current, _| {
            if current == phase {
                println!("{READY}");
                std::io::stdout().flush()?;
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)?;
                panic!("supervisor must kill the blocked child");
            }
            Ok(())
        });
        panic!("supervised checkpoint not reached");
    }
    struct Supervised(Child);
    impl Drop for Supervised {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn snapshot(path: &Path) -> Vec<(PathBuf, u64, u32, Vec<u8>)> {
        fn visit(root: &Path, path: &Path, result: &mut Vec<(PathBuf, u64, u32, Vec<u8>)>) {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            assert!(!metadata.is_symlink());
            result.push((
                path.strip_prefix(root).unwrap().into(),
                metadata.ino(),
                metadata.mode(),
                if metadata.is_file() {
                    std::fs::read(path).unwrap()
                } else {
                    vec![]
                },
            ));
            if metadata.is_dir() {
                for entry in std::fs::read_dir(path).unwrap() {
                    visit(root, &entry.unwrap().path(), result);
                }
            }
        }
        let mut result = vec![];
        visit(path, path, &mut result);
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }
    for phase in ["before", "after"] {
        let root = Root::new();
        let original_plan = root.plan(&["codex"]);
        let destination = original_plan.destinations()[0].path().unwrap();
        let parent = destination.parent().unwrap();
        let unrelated = parent.join(".ask-stage-unrelated");
        std::fs::create_dir(&unrelated).unwrap();
        std::fs::write(unrelated.join("sentinel"), b"not operation-owned").unwrap();
        let unrelated_before = snapshot(&unrelated);
        let mut child = Supervised(Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "executor::apply::native::tests::process_kill_at_commit_boundary_preserves_payload_and_unowned_remnants", "--nocapture"])
            .env(CHILD_ROOT, &root.0).env(CHILD_PHASE, phase)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit())
            .spawn().unwrap());
        let stdout = child.0.stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines() {
                match line {
                    Ok(line) if line == READY => {
                        let _ = sender.send(());
                        return;
                    }
                    Ok(_) => (),
                    Err(_) => return,
                }
            }
        });
        receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("trusted child must reach bounded commit checkpoint");
        reader.join().unwrap();
        assert_eq!(destination.exists(), phase == "after");
        child.0.kill().unwrap();
        let status = child.0.wait().unwrap();
        assert_eq!(status.signal(), Some(libc::SIGKILL));
        let orphan = std::fs::read_dir(parent)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path != &unrelated
                    && path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(".ask-stage-")
            })
            .expect("killed child leaves a private stage without cleanup");
        let orphan_before = snapshot(&orphan);
        if phase == "before" {
            assert_eq!(
                std::fs::read(orphan.join("sample/resource")).unwrap(),
                b"original"
            );
        } else {
            assert_eq!(std::fs::read_dir(&orphan).unwrap().count(), 0);
        }
        let installed_inode = std::fs::metadata(destination).ok().map(|meta| meta.ino());
        let fresh = root.existing_plan(&["codex"]);
        let report = super::super::apply_plan(&fresh, ApplyOptions::default());
        assert!(report.is_successful(), "{report:#?}");
        assert_eq!(
            report.outcomes[0].status,
            if phase == "before" {
                OutcomeStatus::Created
            } else {
                OutcomeStatus::Unchanged
            }
        );
        assert_eq!(
            std::fs::read(destination.join("resource")).unwrap(),
            b"original"
        );
        assert_eq!(
            std::fs::read(destination.join("SKILL.md")).unwrap(),
            std::fs::read(root.0.join("source/sample/SKILL.md")).unwrap()
        );
        if let Some(inode) = installed_inode {
            assert_eq!(std::fs::metadata(destination).unwrap().ino(), inode);
        }
        assert_eq!(
            snapshot(&orphan),
            orphan_before,
            "fresh retry must not adopt orphan cleanup authority"
        );
        assert_eq!(snapshot(&unrelated), unrelated_before);
    }
}

#[test]
fn native_copy_preserves_unicode_spaces_and_read_only_source_intent() {
    use std::os::unix::fs::PermissionsExt;
    let root = Root::new();
    let _ = root.plan(&["codex"]);
    let source = root.0.join("source/sample");
    let resource = source.join("reference café 中.txt");
    std::fs::write(&resource, b"inert original Unicode resource").unwrap();
    std::fs::set_permissions(&resource, std::fs::Permissions::from_mode(0o444)).unwrap();
    let directory = source.join("empty directory 中");
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555)).unwrap();
    let plan = root.existing_plan(&["codex"]);
    let report = super::super::apply_plan(&plan, ApplyOptions::default());
    assert!(report.is_successful(), "{report:#?}");
    let destination = plan.destinations()[0].path().unwrap();
    assert_eq!(
        std::fs::read(destination.join(resource.file_name().unwrap())).unwrap(),
        b"inert original Unicode resource"
    );
    assert_eq!(
        std::fs::metadata(destination.join(resource.file_name().unwrap()))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o444
    );
    assert_eq!(
        std::fs::metadata(destination.join(directory.file_name().unwrap()))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o555
    );
    let fresh = root.existing_plan(&["codex"]);
    let unchanged = super::super::apply_plan(&fresh, ApplyOptions::default());
    assert_eq!(unchanged.outcomes[0].status, OutcomeStatus::Unchanged);
    // Restore only fixture-owned directories so ordinary teardown can remove them.
    for path in [directory, destination.join("empty directory 中")] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_tmpfs_copy_unchanged_and_conflict_use_real_native_filesystem() {
    let root = Root::new_in(Path::new("/dev/shm"));
    let descriptor = std::fs::File::open(&root.0).unwrap();
    assert_eq!(
        rustix::fs::fstatfs(&descriptor).unwrap().f_type as u64,
        0x0102_1994,
        "required /dev/shm fixture must actually be tmpfs; no silent substitute"
    );
    let plan = root.plan(&["codex"]);
    let destination = plan.destinations()[0].path().unwrap();
    let created = super::super::apply_plan(&plan, ApplyOptions::default());
    assert!(created.is_successful(), "{created:#?}");
    assert_eq!(created.outcomes[0].status, OutcomeStatus::Created);
    let fresh = root.existing_plan(&["codex"]);
    let unchanged = super::super::apply_plan(&fresh, ApplyOptions::default());
    assert_eq!(unchanged.outcomes[0].status, OutcomeStatus::Unchanged);
    std::fs::write(destination.join("resource"), b"manual edit survives").unwrap();
    let conflict = root.existing_plan(&["codex"]);
    assert!(!conflict.is_applicable());
    let refused = super::super::apply_plan(&conflict, ApplyOptions::default());
    assert!(refused.preflight_error.is_some());
    assert_eq!(
        std::fs::read(destination.join("resource")).unwrap(),
        b"manual edit survives"
    );
    no_stages(destination.parent().unwrap());
}
