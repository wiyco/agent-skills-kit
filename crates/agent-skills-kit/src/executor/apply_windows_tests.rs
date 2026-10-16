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
