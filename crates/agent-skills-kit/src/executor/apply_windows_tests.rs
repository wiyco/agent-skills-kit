use super::*;
use crate::{
    catalog::{Catalog, Scope},
    planner::{InstallRequest, PlanLimits, plan_install},
    resolver::{InstallContext, native_platform},
    validation::{InventoryLimits, inventory_skill},
};
use std::{
    fs,
    os::windows::{ffi::OsStringExt, fs::OpenOptionsExt},
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES, SYNCHRONIZE,
};

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    context: InstallContext,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = (0..100)
            .find_map(|_| {
                let path = std::env::temp_dir().join(format!(
                    "ask-win-copy-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&path) {
                    Ok(()) => Some(path),
                    Err(observed) if observed.kind() == io::ErrorKind::AlreadyExists => None,
                    Err(observed) => panic!("owned copy fixture: {observed}"),
                }
            })
            .expect("fixture collision budget");
        let source = root.join("source 中/sample");
        fs::create_dir_all(source.join("empty folder")).unwrap();
        fs::write(source.join("SKILL.md"), b"---\nname: sample\ndescription: Original inert Windows copy fixture\ncustom: exact bytes\n---\nNever execute payloads.\n").unwrap();
        fs::write(
            source.join("reference café.txt"),
            b"original resource bytes",
        )
        .unwrap();
        let mut context = InstallContext::new(native_platform().unwrap());
        context.user_home = Some(root.join("home with spaces"));
        context.project_root = Some(root.join("project"));
        fs::create_dir(context.user_home.as_ref().unwrap()).unwrap();
        fs::create_dir(context.project_root.as_ref().unwrap()).unwrap();
        Self {
            root,
            source,
            context,
        }
    }
    fn plan(&self, targets: &[&str], scope: Scope) -> InstallPlan {
        let sources = [inventory_skill(&self.source, InventoryLimits::default()).unwrap()];
        plan_install(
            &Catalog::bundled().unwrap(),
            InstallRequest {
                skills: &sources,
                target_ids: targets,
                scope,
                context: &self.context,
                limits: PlanLimits::default(),
            },
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fn prepare(path: &Path) {
            let metadata = fs::symlink_metadata(path).unwrap();
            if metadata.file_type().is_symlink() {
                return;
            }
            if metadata.is_dir() {
                for entry in fs::read_dir(path).unwrap() {
                    prepare(&entry.unwrap().path());
                }
            }
            if metadata.permissions().readonly() {
                let file = fs::OpenOptions::new()
                    .access_mode(FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE)
                    .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                    .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                    .open(path)
                    .unwrap();
                native::set_readonly(&file, false).unwrap();
            }
        }
        prepare(&self.root);
        fs::remove_dir_all(&self.root).expect("owned copy fixture teardown");
    }
}
fn apply(
    plan: &InstallPlan,
    options: ApplyOptions,
    hook: &mut impl FnMut(Phase, &Path) -> io::Result<()>,
) -> InstallReport {
    let mut report = initial_report(plan);
    if preflight(plan, options, &mut report) {
        apply_with_hook(plan, options, &mut report, hook);
    }
    report
}
fn stages(root: &Path) -> Vec<PathBuf> {
    let mut result = vec![];
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(".ask-stage-")
        {
            result.push(entry.path());
        } else if entry.file_type().unwrap().is_dir() {
            result.extend(stages(&entry.path()));
        }
    }
    result
}
#[test]
fn windows_candidate_final_byte_check_rejects_same_length_stale_timestamp_tampering() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_BASIC_INFO, FileBasicInfo, SetFileInformationByHandle,
    };
    let fixture = Fixture::new();
    let plan = fixture.plan(&["codex"], Scope::User);
    let mut reached = false;
    let report = apply(&plan, ApplyOptions::default(), &mut |phase, path| {
        if phase == Phase::AfterStage {
            reached = true;
            let resource = path.join("sample/reference café.txt");
            let file = fs::OpenOptions::new()
                .access_mode(FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&resource)?;
            let before = native::stamp(&file)?;
            let mut bytes = fs::read(&resource)?;
            bytes[0] ^= 1;
            fs::write(&resource, bytes)?;
            let basic = FILE_BASIC_INFO {
                LastWriteTime: before.modified,
                ChangeTime: before.changed,
                ..Default::default()
            };
            // SAFETY: owned stage fixture only, live attribute-writer handle and
            // exact SDK class/layout. Preserve length and restore observed epochs
            // deliberately so metadata comparison cannot substitute for byte proof.
            assert_ne!(
                unsafe {
                    SetFileInformationByHandle(
                        file.as_raw_handle(),
                        FileBasicInfo,
                        (&basic as *const FILE_BASIC_INFO).cast(),
                        std::mem::size_of::<FILE_BASIC_INFO>() as u32,
                    )
                },
                0
            );
            assert_eq!(native::stamp(&file)?, before);
        }
        Ok(())
    });
    assert!(reached);
    assert!(
        matches!(
            report.outcomes[0].status,
            OutcomeStatus::Failed(ApplyError {
                kind: ApplyErrorKind::StageChanged,
                ..
            })
        ),
        "{report:?}"
    );
    assert!(!plan.destinations()[0].path().unwrap().exists());
    assert!(stages(&fixture.root).is_empty(), "{report:?}");
    assert_eq!(
        fs::read(fixture.source.join("reference café.txt")).unwrap(),
        b"original resource bytes"
    );
}

#[test]
fn windows_candidate_copies_original_native_names_and_repeats_unchanged() {
    for scope in [Scope::User, Scope::Project] {
        let fixture = Fixture::new();
        let opaque = OsString::from_wide(&[b'r' as u16, 0xd800]);
        fs::write(fixture.source.join(&opaque), b"opaque native bytes").unwrap();
        let sentinel = fixture.root.join("must-not-run");
        fs::write(
            fixture.source.join("runme.ps1"),
            format!("New-Item '{}'\n", sentinel.display()),
        )
        .unwrap();
        let plan = fixture.plan(&["opencode", "codex-cli", "claude-code", "codex"], scope);
        assert!(plan.is_applicable(), "{plan:?}");
        let report = apply(&plan, ApplyOptions::default(), &mut |_, _| Ok(()));
        assert!(report.is_successful(), "{report:?}");
        assert_eq!(report.outcomes.len(), 3);
        for row in &report.outcomes {
            assert_eq!(row.status, OutcomeStatus::Created);
            let path = row.path.as_ref().unwrap();
            for name in [
                OsString::from("SKILL.md"),
                OsString::from("reference café.txt"),
                opaque.clone(),
                OsString::from("runme.ps1"),
            ] {
                assert_eq!(
                    fs::read(path.join(&name)).unwrap(),
                    fs::read(fixture.source.join(&name)).unwrap()
                );
            }
            assert!(path.join("empty folder").is_dir());
        }
        assert!(!sentinel.exists());
        assert!(stages(&fixture.root).is_empty());
        let repeat = fixture.plan(&["claude-code", "codex", "opencode"], scope);
        let report = apply(&repeat, ApplyOptions::default(), &mut |_, _| Ok(()));
        assert!(report.is_successful(), "{report:?}");
        assert!(
            report
                .outcomes
                .iter()
                .all(|row| row.status == OutcomeStatus::Unchanged)
        );
    }
}
#[test]
fn windows_candidate_preserves_read_only_intent() {
    let fixture = Fixture::new();
    for path in [
        fixture.source.clone(),
        fixture.source.join("empty folder"),
        fixture.source.join("reference café.txt"),
    ] {
        let file = fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)
            .unwrap();
        native::set_readonly(&file, true).unwrap();
    }
    let plan = fixture.plan(&["codex"], Scope::User);
    let report = apply(&plan, ApplyOptions::default(), &mut |_, _| Ok(()));
    assert!(report.is_successful(), "{report:?}");
    let path = report.outcomes[0].path.as_ref().unwrap();
    for relative in ["", "empty folder", "reference café.txt"] {
        assert!(
            fs::metadata(path.join(relative))
                .unwrap()
                .permissions()
                .readonly()
        );
    }
    let repeat = fixture.plan(&["codex"], Scope::User);
    assert_eq!(repeat.destinations()[0].action(), PlannedAction::Unchanged);
}
#[test]
fn windows_candidate_reports_committed_permission_failure_and_keeps_payload() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&["codex"], Scope::User);
    let report = apply(&plan, ApplyOptions::default(), &mut |phase, _| {
        if phase == Phase::BeforePermissions {
            Err(io::Error::from_raw_os_error(5))
        } else {
            Ok(())
        }
    });
    assert!(!report.is_successful());
    assert!(
        matches!(
            report.outcomes[0].status,
            OutcomeStatus::CommittedWithPermissionFailure(_)
        ),
        "{report:?}"
    );
    let path = report.outcomes[0].path.as_ref().unwrap();
    assert_eq!(
        fs::read(path.join("SKILL.md")).unwrap(),
        fs::read(fixture.source.join("SKILL.md")).unwrap()
    );
    assert!(stages(&fixture.root).is_empty(), "{report:?}");
    use crate::receipt::*;
    let receipt = Receipt::from_report(
        &plan,
        &report,
        OperationId::new("candidate-committed-permissions").unwrap(),
        ReceiptLimits::default(),
    )
    .unwrap();
    let row = &receipt.document().destinations[0];
    assert_eq!(row.status, ReceiptStatus::Failed);
    assert_eq!(
        row.failure.as_ref().unwrap().operation.as_deref(),
        Some("payload committed; destination permission finalization failed")
    );
    let json = receipt.to_json(ReceiptLimits::default()).unwrap();
    assert_eq!(
        Receipt::from_json(&json, ReceiptLimits::default())
            .unwrap()
            .document(),
        receipt.document()
    );
}
#[test]
fn windows_candidate_copy_fault_cleans_only_owned_stage_and_retains_new_parents() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&["codex"], Scope::User);
    let sentinel = fixture
        .context
        .user_home
        .as_ref()
        .unwrap()
        .join(".ask-stage-unrelated");
    fs::create_dir(&sentinel).unwrap();
    fs::write(sentinel.join("sentinel"), b"untouched").unwrap();
    let report = apply(&plan, ApplyOptions::default(), &mut |phase, _| {
        if phase == Phase::AfterWriteChunk {
            Err(io::Error::from_raw_os_error(112))
        } else {
            Ok(())
        }
    });
    assert!(!report.is_successful());
    assert!(
        matches!(report.outcomes[0].status, OutcomeStatus::Failed(_)),
        "{report:?}"
    );
    assert!(!plan.destinations()[0].path().unwrap().exists());
    assert_eq!(fs::read(sentinel.join("sentinel")).unwrap(), b"untouched");
    assert_eq!(stages(&fixture.root), [sentinel]);
    assert!(
        report
            .cleanup_errors
            .iter()
            .all(|failure| failure.kind == ApplyErrorKind::CreatedParentRetained),
        "{report:?}"
    );
}

fn set_fixture_dacl(path: &Path, sddl: &str) {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
        },
        Storage::FileSystem::{READ_CONTROL, WRITE_DAC},
    };
    let file = fs::OpenOptions::new()
        .access_mode(READ_CONTROL | WRITE_DAC)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .unwrap();
    let text = sddl.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut pointer = std::ptr::null_mut();
    // SAFETY: trusted original fixture SDDL, terminated input and one API allocation.
    assert_ne!(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut pointer,
                std::ptr::null_mut(),
            )
        },
        0
    );
    assert!(!pointer.is_null());
    struct Descriptor(*mut core::ffi::c_void);
    impl Drop for Descriptor {
        fn drop(&mut self) {
            // SAFETY: unique allocation from successful SDDL conversion.
            unsafe {
                LocalFree(self.0);
            }
        }
    }
    let descriptor = Descriptor(pointer);
    use std::os::windows::io::AsRawHandle;
    // SAFETY: exact held owned fixture object with WRITE_DAC and live descriptor.
    let status = unsafe {
        windows_sys::Wdk::Storage::FileSystem::NtSetSecurityObject(
            file.as_raw_handle(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor.0,
        )
    };
    assert_eq!(status, 0, "set owned fixture DACL: {status:#x}");
}
fn fixture_dacl(path: &Path) -> String {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertSecurityDescriptorToStringSecurityDescriptorW, SDDL_REVISION_1,
            },
            DACL_SECURITY_INFORMATION,
        },
    };
    let file = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .unwrap();
    let bytes = security::descriptor(&file).unwrap();
    let mut aligned = vec![0u64; bytes.len().div_ceil(8)];
    // SAFETY: aligned initialized allocation has sufficient byte capacity.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), aligned.as_mut_ptr().cast(), bytes.len());
    }
    let mut text = std::ptr::null_mut();
    let mut length = 0;
    // SAFETY: valid bounded SDK descriptor in aligned storage and API-allocated output.
    assert_ne!(
        unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                aligned.as_mut_ptr().cast(),
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut text,
                &mut length,
            )
        },
        0
    );
    assert!(!text.is_null());
    struct Text(*mut u16);
    impl Drop for Text {
        fn drop(&mut self) {
            // SAFETY: unique SDK conversion allocation.
            unsafe {
                LocalFree(self.0.cast());
            }
        }
    }
    let text = Text(text);
    assert!((1..=65536).contains(&length));
    // SAFETY: reported allocation bound; stop at the first returned NUL.
    let units = unsafe { std::slice::from_raw_parts(text.0, length as usize) };
    let end = units.iter().position(|unit| *unit == 0).unwrap();
    String::from_utf16(&units[..end]).unwrap()
}
#[test]
fn windows_actual_create_permission_denial_keeps_prior_success_and_honors_stop_policy() {
    for stop in [true, false] {
        let fixture = Fixture::new();
        let denied = fixture
            .context
            .user_home
            .as_ref()
            .unwrap()
            .join(".claude/skills");
        fs::create_dir_all(&denied).unwrap();
        let token = creation::Token::effective().unwrap();
        let sid = token.user_sid().unwrap();
        // Deny only FILE_ADD_SUBDIRECTORY, keeping enumeration/READ_CONTROL valid.
        set_fixture_dacl(
            &denied,
            &format!("O:{sid}D:P(D;;0x4;;;{sid})(A;;FA;;;{sid})"),
        );
        let before = fixture_dacl(&denied);
        let plan = fixture.plan(&["codex", "claude-code", "opencode"], Scope::User);
        assert!(plan.is_applicable());
        let report = apply(
            &plan,
            ApplyOptions {
                stop_on_failure: stop,
                ..ApplyOptions::default()
            },
            &mut |_, _| Ok(()),
        );
        let row = |target: &str| {
            report
                .outcomes
                .iter()
                .find(|row| {
                    row.attributions
                        .iter()
                        .any(|attribution| attribution.target_id == target)
                })
                .unwrap()
        };
        assert_eq!(row("codex").status, OutcomeStatus::Created, "{report:?}");
        assert!(
            matches!(
                row("claude-code").status,
                OutcomeStatus::Failed(ApplyError {
                    kind: ApplyErrorKind::Io {
                        kind: io::ErrorKind::PermissionDenied,
                        ..
                    },
                    ..
                })
            ),
            "{report:?}"
        );
        assert_eq!(
            row("opencode").status,
            if stop {
                OutcomeStatus::NotAttempted
            } else {
                OutcomeStatus::Created
            },
            "{report:?}"
        );
        assert_eq!(
            fs::read(row("codex").path.as_ref().unwrap().join("SKILL.md")).unwrap(),
            fs::read(fixture.source.join("SKILL.md")).unwrap()
        );
        assert!(!denied.join("sample").exists());
        assert_eq!(before, fixture_dacl(&denied));
        assert!(stages(&fixture.root).is_empty());
    }
}
#[test]
fn windows_copy_does_not_import_source_acl_grants_and_conflicts_preserve_edits() {
    let fixture = Fixture::new();
    let resource = fixture.source.join("reference café.txt");
    let sid = creation::Token::effective().unwrap().user_sid().unwrap();
    set_fixture_dacl(&resource, &format!("O:{sid}D:P(A;;FA;;;{sid})(A;;FR;;;WD)"));
    let source_security = fixture_dacl(&resource);
    assert!(source_security.contains(";;;WD)"));
    let plan = fixture.plan(&["codex"], Scope::User);
    let report = apply(&plan, ApplyOptions::default(), &mut |_, _| Ok(()));
    assert!(report.is_successful(), "{report:?}");
    let target = report.outcomes[0].path.as_ref().unwrap();
    assert!(!fixture_dacl(&target.join("reference café.txt")).contains(";;;WD)"));
    assert_eq!(source_security, fixture_dacl(&resource));
    fs::write(target.join("reference café.txt"), b"preserved manual edit").unwrap();
    let conflict = fixture.plan(&["codex"], Scope::User);
    assert_eq!(conflict.destinations()[0].action(), PlannedAction::Conflict);
    let report = apply(&conflict, ApplyOptions::default(), &mut |_, _| Ok(()));
    assert!(report.preflight_error.is_some());
    assert_eq!(report.outcomes[0].status, OutcomeStatus::NotAttempted);
    assert_eq!(
        fs::read(target.join("reference café.txt")).unwrap(),
        b"preserved manual edit"
    );
    assert!(stages(&fixture.root).is_empty());
}
#[test]
fn windows_last_moment_file_empty_directory_and_reparse_winners_are_preserved() {
    for scenario in ["file", "directory", "reparse"] {
        let fixture = Fixture::new();
        let plan = fixture.plan(&["codex"], Scope::User);
        let outside = fixture.root.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"untouched").unwrap();
        let mut reached = false;
        let report = apply(&plan, ApplyOptions::default(), &mut |phase, path| {
            if phase == Phase::BeforeCommit {
                reached = true;
                match scenario {
                    "file" => fs::write(path, b"winner")?,
                    "directory" => fs::create_dir(path)?,
                    "reparse" => std::os::windows::fs::symlink_dir(&outside, path)
                        .expect("required winner reparse fixture privilege"),
                    _ => unreachable!(),
                }
            }
            Ok(())
        });
        assert!(reached);
        assert!(
            matches!(report.outcomes[0].status, OutcomeStatus::Failed(_)),
            "{report:?}"
        );
        let path = plan.destinations()[0].path().unwrap();
        match scenario {
            "file" => assert_eq!(fs::read(path).unwrap(), b"winner"),
            "directory" => assert_eq!(fs::read_dir(path).unwrap().count(), 0),
            "reparse" => {
                assert!(fs::symlink_metadata(path).unwrap().file_type().is_symlink());
                fs::remove_dir(path).unwrap();
            }
            _ => unreachable!(),
        }
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"untouched");
        assert!(stages(&fixture.root).is_empty(), "{report:?}");
    }
}
#[test]
fn windows_source_and_stage_mutations_fail_before_commit_without_touching_outside_data() {
    for scenario in [
        "source-bytes",
        "source-reparse",
        "stage-bytes",
        "stage-extra",
    ] {
        let fixture = Fixture::new();
        let plan = fixture.plan(&["codex"], Scope::User);
        let outside = fixture.root.join("outside-sentinel");
        fs::write(&outside, b"outside unchanged").unwrap();
        let mut reached = false;
        let report = apply(&plan, ApplyOptions::default(), &mut |phase, path| {
            if !reached
                && scenario == "source-bytes"
                && phase == Phase::AfterSourceRead
                && path.ends_with("reference café.txt")
            {
                reached = true;
                fs::write(path, b"modified resource bytes")?;
            }
            if !reached
                && scenario == "source-reparse"
                && phase == Phase::BeforeCopyFile
                && path.ends_with("reference café.txt")
            {
                reached = true;
                fs::remove_file(path)?;
                std::os::windows::fs::symlink_file(&outside, path)
                    .expect("required source-swap reparse privilege");
            }
            if !reached && phase == Phase::AfterStage && scenario.starts_with("stage-") {
                reached = true;
                let payload = path.join("sample");
                if scenario == "stage-bytes" {
                    fs::write(
                        payload.join("reference café.txt"),
                        b"modified resource bytes",
                    )?;
                } else {
                    fs::write(payload.join("unowned extra"), b"foreign entry")?;
                }
            }
            Ok(())
        });
        assert!(reached, "{scenario}");
        assert!(
            matches!(report.outcomes[0].status, OutcomeStatus::Failed(_)),
            "{scenario}: {report:?}"
        );
        assert!(!plan.destinations()[0].path().unwrap().exists());
        assert_eq!(fs::read(outside).unwrap(), b"outside unchanged");
        if scenario == "stage-extra" {
            assert!(!report.cleanup_errors.is_empty());
            let retained = stages(&fixture.root);
            assert_eq!(retained.len(), 1, "{report:?}");
            assert_eq!(
                fs::read(retained[0].join("sample/unowned extra")).unwrap(),
                b"foreign entry"
            );
        } else {
            assert!(stages(&fixture.root).is_empty(), "{scenario}: {report:?}");
        }
    }
}
#[test]
fn windows_two_installs_race_at_native_commit_with_exactly_one_winner() {
    use std::{sync::mpsc, thread, time::Duration};
    let fixture = Fixture::new();
    fs::create_dir_all(
        fixture
            .context
            .user_home
            .as_ref()
            .unwrap()
            .join(".agents/skills"),
    )
    .unwrap();
    let first = fixture.plan(&["codex"], Scope::User);
    let second = fixture.plan(&["codex"], Scope::User);
    let (ready_tx, ready_rx) = mpsc::channel();
    let mut releases = vec![];
    let mut workers = vec![];
    for plan in [first, second] {
        let ready_tx = ready_tx.clone();
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        releases.push(release_tx);
        workers.push(thread::spawn(move || {
            apply(&plan, ApplyOptions::default(), &mut |phase, _| {
                if phase == Phase::BeforeCommit {
                    ready_tx
                        .send(())
                        .map_err(|_| io::Error::other("commit readiness disconnected"))?;
                    release_rx
                        .recv_timeout(Duration::from_secs(10))
                        .map_err(|_| {
                            io::Error::new(io::ErrorKind::TimedOut, "bounded commit release")
                        })?;
                }
                Ok(())
            })
        }));
    }
    drop(ready_tx);
    let readiness = (0..2)
        .map(|_| ready_rx.recv_timeout(Duration::from_secs(10)))
        .collect::<Vec<_>>();
    for release in releases {
        let _ = release.send(());
    }
    let reports = workers
        .into_iter()
        .map(|worker| worker.join())
        .collect::<Vec<_>>();
    assert!(readiness.iter().all(Result::is_ok), "{readiness:?}");
    let reports = reports.into_iter().map(Result::unwrap).collect::<Vec<_>>();
    assert_eq!(
        reports
            .iter()
            .filter(|report| report.outcomes[0].status == OutcomeStatus::Created)
            .count(),
        1,
        "{reports:?}"
    );
    assert_eq!(
        reports
            .iter()
            .filter(|report| matches!(report.outcomes[0].status, OutcomeStatus::Failed(_)))
            .count(),
        1,
        "{reports:?}"
    );
    assert!(
        reports
            .iter()
            .all(|report| report.cleanup_errors.is_empty()),
        "{reports:?}"
    );
    let path = reports[0].outcomes[0].path.as_ref().unwrap();
    assert_eq!(
        fs::read(path.join("SKILL.md")).unwrap(),
        fs::read(fixture.source.join("SKILL.md")).unwrap()
    );
    assert!(stages(&fixture.root).is_empty());
}

#[test]
fn windows_committed_permission_failure_honors_both_batch_stop_policies() {
    for stop in [true, false] {
        let fixture = Fixture::new();
        let plan = fixture.plan(&["codex", "claude-code", "opencode"], Scope::User);
        let mut reached = false;
        let report = apply(
            &plan,
            ApplyOptions {
                stop_on_failure: stop,
                ..ApplyOptions::default()
            },
            &mut |phase, path| {
                if phase == Phase::BeforePermissions && path.ends_with(".claude/skills/sample") {
                    reached = true;
                    return Err(io::Error::from_raw_os_error(5));
                }
                Ok(())
            },
        );
        assert!(reached);
        assert!(!report.is_successful());
        let row = |target: &str| {
            report
                .outcomes
                .iter()
                .find(|row| {
                    row.attributions
                        .iter()
                        .any(|attribution| attribution.target_id == target)
                })
                .unwrap()
        };
        assert_eq!(row("codex").status, OutcomeStatus::Created);
        assert!(
            matches!(
                row("claude-code").status,
                OutcomeStatus::CommittedWithPermissionFailure(_)
            ),
            "{report:?}"
        );
        assert!(row("claude-code").status.payload_was_committed());
        assert_eq!(
            row("opencode").status,
            if stop {
                OutcomeStatus::NotAttempted
            } else {
                OutcomeStatus::Created
            }
        );
        let committed = row("claude-code").path.as_ref().unwrap();
        assert_eq!(
            fs::read(committed.join("SKILL.md")).unwrap(),
            fs::read(fixture.source.join("SKILL.md")).unwrap()
        );
        assert!(fixture_dacl(committed).starts_with("D:P"));
        assert!(report.cleanup_errors.is_empty(), "{report:?}");
        assert!(stages(&fixture.root).is_empty());
    }
}
#[test]
fn windows_read_only_stage_fault_preserves_source_and_cleans_recorded_objects() {
    let fixture = Fixture::new();
    let resource = fixture.source.join("reference café.txt");
    let permissions = fs::metadata(&resource).unwrap().permissions();
    let mut readonly = permissions.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&resource, readonly).unwrap();
    let plan = fixture.plan(&["codex"], Scope::User);
    let mut reached = false;
    let report = apply(&plan, ApplyOptions::default(), &mut |phase, _| {
        if phase == Phase::AfterStage {
            reached = true;
            return Err(io::Error::from_raw_os_error(1117));
        }
        Ok(())
    });
    assert!(reached);
    assert!(
        matches!(report.outcomes[0].status, OutcomeStatus::Failed(_)),
        "{report:?}"
    );
    assert!(!plan.destinations()[0].path().unwrap().exists());
    assert!(stages(&fixture.root).is_empty(), "{report:?}");
    assert_eq!(fs::read(&resource).unwrap(), b"original resource bytes");
    assert!(fs::metadata(&resource).unwrap().permissions().readonly());
    fs::set_permissions(&resource, permissions).unwrap();
}
