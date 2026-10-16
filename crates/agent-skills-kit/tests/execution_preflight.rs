#![cfg(any(target_os = "linux", target_os = "macos", windows))]

use agent_skills_kit::{catalog::*, executor::*, planner::*, resolver::*, validation::*};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

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
                let root = std::env::temp_dir().join(format!(
                    "ask-preflight-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&root) {
                    Ok(()) => Some(root),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                    Err(error) => panic!("exclusive fixture: {error}"),
                }
            })
            .expect("fixture collision budget");
        let source = root.join("source/sample");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            b"---\nname: sample\ndescription: inert preflight fixture\n---\n",
        )
        .unwrap();
        fs::write(source.join("resource"), b"original resource").unwrap();
        let mut context = InstallContext::new(native_platform().unwrap());
        context.user_home = Some(root.join("home"));
        fs::create_dir(context.user_home.as_ref().unwrap()).unwrap();
        Self {
            root,
            source,
            context,
        }
    }
    fn destination(&self) -> PathBuf {
        self.root.join("home/.agents/skills/sample")
    }
    fn existing(&self) {
        fs::create_dir_all(self.destination()).unwrap();
        for name in ["SKILL.md", "resource"] {
            fs::copy(self.source.join(name), self.destination().join(name)).unwrap();
        }
    }
    fn plan(&self) -> InstallPlan {
        let sources = [inventory_skill(&self.source, InventoryLimits::default()).unwrap()];
        plan_install(
            &Catalog::bundled().unwrap(),
            InstallRequest {
                skills: &sources,
                target_ids: &["codex"],
                scope: Scope::User,
                context: &self.context,
                limits: PlanLimits::default(),
            },
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("owned fixture cleanup");
    }
}

fn tree(root: &Path) -> Vec<(PathBuf, Vec<u8>, u32)> {
    fn walk(root: &Path, path: &Path, output: &mut Vec<(PathBuf, Vec<u8>, u32)>) {
        let metadata = fs::symlink_metadata(path).unwrap();
        let bytes = if metadata.is_file() {
            fs::read(path).unwrap()
        } else if metadata.file_type().is_symlink() {
            fs::read_link(path)
                .unwrap()
                .as_os_str()
                .as_encoded_bytes()
                .to_vec()
        } else {
            Vec::new()
        };
        output.push((path.strip_prefix(root).unwrap().to_owned(), bytes, {
            #[cfg(unix)]
            {
                metadata.mode()
            }
            #[cfg(windows)]
            {
                metadata.file_attributes()
            }
        }));
        if metadata.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                walk(root, &entry.unwrap().path(), output);
            }
        }
    }
    let mut result = Vec::new();
    walk(root, root, &mut result);
    result.sort_by(|a, b| a.0.cmp(&b.0));
    result
}

#[test]
fn successful_preflight_is_read_only_for_create_and_unchanged() {
    for existing in [false, true] {
        let fixture = Fixture::new();
        if existing {
            fixture.existing();
        }
        let plan = fixture.plan();
        let before = tree(&fixture.root);
        let report = verify_plan(&plan, VerificationLimits::default()).unwrap();
        assert_eq!(report.sources_checked, 1);
        assert_eq!(report.destinations_checked, 1);
        assert_eq!(report.entries, if existing { 4 } else { 2 });
        assert_eq!(
            report.read_bytes,
            plan.sources()[0].inventory().total_bytes() * if existing { 2 } else { 1 }
        );
        assert_eq!(tree(&fixture.root), before);
    }
}

#[cfg(unix)]
#[test]
fn modified_and_replaced_sources_are_never_adopted() {
    for scenario in ["manifest", "resource", "mode", "replace", "link", "missing"] {
        let fixture = Fixture::new();
        let plan = fixture.plan();
        let resource = fixture.source.join("resource");
        match scenario {
            "manifest" => fs::write(
                fixture.source.join("SKILL.md"),
                b"---\nname: sample\ndescription: changed\n---\n",
            )
            .unwrap(),
            "resource" => fs::write(&resource, b"changed resource").unwrap(),
            "mode" => fs::set_permissions(&resource, fs::Permissions::from_mode(0o600)).unwrap(),
            "replace" => {
                fs::rename(&resource, fixture.root.join("saved")).unwrap();
                fs::write(&resource, b"original resource").unwrap();
            }
            "link" => {
                fs::remove_file(&resource).unwrap();
                symlink(fixture.source.join("SKILL.md"), &resource).unwrap();
            }
            "missing" => fs::remove_file(&resource).unwrap(),
            _ => unreachable!(),
        }
        let before = tree(&fixture.root);
        let error = verify_plan(&plan, VerificationLimits::default()).unwrap_err();
        assert!(
            matches!(
                error.kind,
                VerificationErrorKind::SourceChanged | VerificationErrorKind::SourceInspection(_)
            ),
            "{scenario}: {error:?}"
        );
        assert_eq!(tree(&fixture.root), before, "{scenario}");
        assert!(!fixture.destination().exists());
    }
}

#[cfg(unix)]
#[test]
fn newly_created_destination_is_not_an_approved_absence() {
    for scenario in ["file", "directory", "link"] {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.destination().parent().unwrap()).unwrap();
        let plan = fixture.plan();
        match scenario {
            "file" => fs::write(fixture.destination(), b"winner").unwrap(),
            "directory" => fs::create_dir(fixture.destination()).unwrap(),
            "link" => symlink(&fixture.source, fixture.destination()).unwrap(),
            _ => unreachable!(),
        }
        let before = tree(&fixture.root);
        let error = verify_plan(&plan, VerificationLimits::default()).unwrap_err();
        assert!(matches!(
            error.kind,
            VerificationErrorKind::DestinationChanged | VerificationErrorKind::Namespace(_)
        ));
        assert_eq!(tree(&fixture.root), before);
    }
}

#[cfg(unix)]
#[test]
fn substituted_parent_is_not_followed() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.destination().parent().unwrap()).unwrap();
    let plan = fixture.plan();
    let selected = fixture.root.join("home/.agents");
    fs::rename(&selected, fixture.root.join("held-parent")).unwrap();
    let outside = fixture.root.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"untouched").unwrap();
    symlink(&outside, &selected).unwrap();
    let before = tree(&fixture.root);
    assert!(verify_plan(&plan, VerificationLimits::default()).is_err());
    assert_eq!(tree(&fixture.root), before);
}

#[cfg(unix)]
#[test]
fn unchanged_payloads_require_the_original_full_snapshot() {
    for scenario in ["bytes", "replacement", "mode", "extra"] {
        let fixture = Fixture::new();
        fixture.existing();
        let plan = fixture.plan();
        let path = fixture.destination().join("resource");
        match scenario {
            "bytes" => fs::write(&path, b"modified resource").unwrap(),
            "replacement" => {
                fs::rename(&path, fixture.root.join("saved")).unwrap();
                fs::write(&path, b"original resource").unwrap();
            }
            "mode" => fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap(),
            "extra" => fs::write(fixture.destination().join("extra"), b"manual").unwrap(),
            _ => unreachable!(),
        }
        let before = tree(&fixture.root);
        let error = verify_plan(&plan, VerificationLimits::default()).unwrap_err();
        assert!(
            matches!(
                error.kind,
                VerificationErrorKind::DestinationChanged
                    | VerificationErrorKind::DestinationInspection(_)
            ),
            "{scenario}: {error:?}"
        );
        assert_eq!(tree(&fixture.root), before);
    }
}

#[test]
fn explicit_limits_and_inapplicable_plans_fail_without_writes() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    let total = plan.sources()[0].inventory().total_bytes();
    let before = tree(&fixture.root);
    let exact = VerificationLimits {
        max_read_bytes: total,
        max_entries: 2,
        ..VerificationLimits::default()
    };
    assert!(verify_plan(&plan, exact).is_ok());
    for limits in [
        VerificationLimits {
            max_read_bytes: 0,
            ..exact
        },
        VerificationLimits {
            max_read_bytes: total - 1,
            ..exact
        },
        VerificationLimits {
            max_entries: 1,
            ..exact
        },
    ] {
        assert!(verify_plan(&plan, limits).is_err());
    }
    assert_eq!(tree(&fixture.root), before);
    fixture.existing();
    fs::write(fixture.destination().join("resource"), b"manual changes").unwrap();
    let conflict = fixture.plan();
    let before = tree(&fixture.root);
    assert_eq!(
        verify_plan(&conflict, VerificationLimits::default())
            .unwrap_err()
            .kind,
        VerificationErrorKind::InapplicablePlan
    );
    assert_eq!(tree(&fixture.root), before);
}

#[cfg(unix)]
#[test]
fn native_copy_creates_exact_payload_and_repeat_is_unchanged() {
    let fixture = Fixture::new();
    let sentinel = fixture.root.join("must-not-run");
    fs::write(
        fixture.source.join("runme.sh"),
        format!("#!/bin/sh\ntouch '{}'\n", sentinel.display()),
    )
    .unwrap();
    fs::set_permissions(
        fixture.source.join("runme.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    fs::create_dir(fixture.source.join("empty")).unwrap();
    let source_file = fs::File::open(fixture.source.join("resource")).unwrap();
    rustix::fs::fsetxattr(
        &source_file,
        "user.ask-copy-test",
        b"must-not-copy",
        rustix::fs::XattrFlags::empty(),
    )
    .unwrap();
    fs::set_permissions(
        fixture.source.join("resource"),
        fs::Permissions::from_mode(0o4750),
    )
    .unwrap();
    assert_ne!(
        fs::metadata(fixture.source.join("resource"))
            .unwrap()
            .mode()
            & 0o4000,
        0,
        "copy fixture requires real setuid bits; use the scoped unsandboxed suite"
    );
    let plan = fixture.plan();
    let report = apply_plan(&plan, ApplyOptions::default());
    assert!(report.is_successful(), "{report:#?}");
    assert_eq!(report.outcomes[0].status, OutcomeStatus::Created);
    assert_eq!(
        fs::read(fixture.destination().join("resource")).unwrap(),
        b"original resource"
    );
    assert!(fixture.destination().join("empty").is_dir());
    assert_eq!(
        fs::metadata(fixture.destination().join("resource"))
            .unwrap()
            .mode()
            & 0o7777,
        0o750
    );
    let output = fs::File::open(fixture.destination().join("resource")).unwrap();
    let mut attribute = [0u8; 64];
    let missing =
        rustix::fs::fgetxattr(&output, "user.ask-copy-test", &mut attribute[..]).unwrap_err();
    #[cfg(target_os = "linux")]
    assert_eq!(missing, rustix::io::Errno::NODATA);
    #[cfg(target_os = "macos")]
    assert_eq!(missing, rustix::io::Errno::NOATTR);
    assert!(
        fs::read_dir(fixture.destination().parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".ask-stage-"))
    );
    let plan = fixture.plan();
    assert_eq!(plan.destinations()[0].action(), PlannedAction::Unchanged);
    let before = tree(&fixture.root);
    let report = apply_plan(&plan, ApplyOptions::default());
    assert!(report.is_successful(), "{report:#?}");
    assert_eq!(report.outcomes[0].status, OutcomeStatus::Unchanged);
    assert_eq!(tree(&fixture.root), before);
    assert!(!sentinel.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn native_copy_preserves_non_utf8_resource_identity() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let fixture = Fixture::new();
    let name = OsStr::from_bytes(b"resource-\xff");
    fs::write(fixture.source.join(name), b"raw native filename").unwrap();
    let plan = fixture.plan();
    let report = apply_plan(&plan, ApplyOptions::default());
    assert!(report.is_successful(), "{report:#?}");
    assert_eq!(
        fs::read(fixture.destination().join(name)).unwrap(),
        b"raw native filename"
    );
    assert_eq!(
        fixture.plan().destinations()[0].action(),
        PlannedAction::Unchanged
    );
}

#[cfg(unix)]
#[test]
fn native_batch_keeps_every_attribution_and_shares_only_owned_parents() {
    let fixture = Fixture::new();
    let other = fixture.root.join("source/other");
    fs::create_dir(&other).unwrap();
    fs::write(
        other.join("SKILL.md"),
        b"---\nname: other\ndescription: another inert fixture\n---\n",
    )
    .unwrap();
    let sources = [
        inventory_skill(&fixture.source, InventoryLimits::default()).unwrap(),
        inventory_skill(&other, InventoryLimits::default()).unwrap(),
    ];
    let plan = plan_install(
        &Catalog::bundled().unwrap(),
        InstallRequest {
            skills: &sources,
            target_ids: &["codex", "codex-cli", "claude-code", "opencode"],
            scope: Scope::User,
            context: &fixture.context,
            limits: PlanLimits::default(),
        },
    )
    .unwrap();
    let source_before = tree(&fixture.root.join("source"));
    let report = apply_plan(&plan, ApplyOptions::default());
    assert!(report.is_successful(), "{report:#?}");
    assert_eq!(report.outcomes.len(), 6);
    assert!(
        report
            .outcomes
            .iter()
            .all(|outcome| outcome.status == OutcomeStatus::Created
                && outcome.attributions.len() == 1)
    );
    assert_eq!(tree(&fixture.root.join("source")), source_before);
    for directory in [
        ".agents/skills",
        ".claude/skills",
        ".config/opencode/skills",
    ] {
        for name in ["sample", "other"] {
            assert!(
                fixture
                    .root
                    .join("home")
                    .join(directory)
                    .join(name)
                    .join("SKILL.md")
                    .is_file()
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn project_copy_uses_only_the_explicit_project_and_native_core_placements() {
    let mut fixture = Fixture::new();
    let project = fixture.root.join("new-project");
    fixture.context.project_root = Some(project.clone());
    let sources = [inventory_skill(&fixture.source, InventoryLimits::default()).unwrap()];
    let source_before = tree(&fixture.source);
    let plan = plan_install(
        &Catalog::bundled().unwrap(),
        InstallRequest {
            skills: &sources,
            target_ids: &["codex", "claude-code", "opencode"],
            scope: Scope::Project,
            context: &fixture.context,
            limits: PlanLimits::default(),
        },
    )
    .unwrap();
    assert!(!project.exists());
    let report = apply_plan(&plan, ApplyOptions::default());
    assert!(report.is_successful(), "{report:#?}");
    assert_eq!(report.outcomes.len(), 3);
    for directory in [".agents/skills", ".claude/skills", ".opencode/skills"] {
        assert_eq!(
            fs::read(project.join(directory).join("sample/SKILL.md")).unwrap(),
            sources[0].metadata().source_bytes()
        );
    }
    assert_eq!(
        fs::read_dir(fixture.context.user_home.as_ref().unwrap())
            .unwrap()
            .count(),
        0
    );
    assert_eq!(tree(&fixture.source), source_before);
}

#[cfg(unix)]
#[test]
fn apply_preflight_never_overwrites_new_or_modified_content() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    fixture.existing();
    fs::write(fixture.destination().join("resource"), b"manual content").unwrap();
    let before = tree(&fixture.root);
    let report = apply_plan(&plan, ApplyOptions::default());
    assert!(report.preflight_error.is_some());
    assert!(
        report
            .outcomes
            .iter()
            .all(|outcome| outcome.status == OutcomeStatus::NotAttempted)
    );
    assert_eq!(tree(&fixture.root), before);
}

#[cfg(target_os = "macos")]
#[test]
fn acl_bearing_parent_is_refused_without_staging_or_permission_changes() {
    let fixture = Fixture::new();
    let parent = fixture.destination().parent().unwrap().to_owned();
    fs::create_dir_all(&parent).unwrap();
    let status = std::process::Command::new("/bin/chmod")
        .args([
            "+a",
            "everyone allow read,write,execute,file_inherit,directory_inherit",
        ])
        .arg(&parent)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "owned ACL fixture must actually be created"
    );
    let plan = fixture.plan();
    let before = tree(&fixture.root);
    let report = apply_plan(&plan, ApplyOptions::default());
    assert!(!report.is_successful());
    assert!(
        matches!(
            report.outcomes[0].status,
            OutcomeStatus::Failed(ApplyError {
                kind: ApplyErrorKind::Unsupported(_),
                ..
            })
        ),
        "{report:#?}"
    );
    assert_eq!(tree(&fixture.root), before);
}

#[cfg(target_os = "linux")]
#[test]
fn default_acl_parent_is_refused_without_modifying_its_acl() {
    let fixture = Fixture::new();
    let parent = fixture.destination().parent().unwrap().to_owned();
    fs::create_dir_all(&parent).unwrap();
    let descriptor = fs::File::open(&parent).unwrap();
    // Original fixture encoding follows Linux UAPI posix_acl_xattr.h version 2:
    // little-endian header and user/group/other entries with undefined IDs.
    // No header/source implementation is copied or executed.
    let mut acl = 2u32.to_le_bytes().to_vec();
    for (tag, permission) in [(1u16, 7u16), (4, 0), (32, 0)] {
        acl.extend_from_slice(&tag.to_le_bytes());
        acl.extend_from_slice(&permission.to_le_bytes());
        acl.extend_from_slice(&u32::MAX.to_le_bytes());
    }
    rustix::fs::fsetxattr(
        &descriptor,
        "system.posix_acl_default",
        &acl,
        rustix::fs::XattrFlags::empty(),
    )
    .expect("native ACL fixture must be supported and actually stored");
    let plan = fixture.plan();
    let before = tree(&fixture.root);
    let report = apply_plan(&plan, ApplyOptions::default());
    assert!(
        matches!(
            report.outcomes[0].status,
            OutcomeStatus::Failed(ApplyError {
                kind: ApplyErrorKind::Unsupported(_),
                ..
            })
        ),
        "{report:#?}"
    );
    let mut current = [0u8; 256];
    let length =
        rustix::fs::fgetxattr(&descriptor, "system.posix_acl_default", &mut current[..]).unwrap();
    assert_eq!(&current[..length], acl);
    assert_eq!(tree(&fixture.root), before);
}

#[cfg(windows)]
#[test]
fn windows_changed_sources_and_unchanged_payloads_fail_reverification() {
    for existing in [false, true] {
        for scenario in ["bytes", "replacement", "readonly", "extra", "link"] {
            let fixture = Fixture::new();
            if existing {
                fixture.existing();
            }
            let plan = fixture.plan();
            let base = if existing {
                fixture.destination()
            } else {
                fixture.source.clone()
            };
            let path = base.join("resource");
            let original_permissions = fs::metadata(&path).unwrap().permissions();
            match scenario {
                "bytes" => fs::write(&path, b"modified resource").unwrap(),
                "replacement" => {
                    fs::rename(&path, fixture.root.join("saved")).unwrap();
                    fs::write(&path, b"original resource").unwrap();
                }
                "readonly" => {
                    let mut changed = original_permissions.clone();
                    changed.set_readonly(true);
                    fs::set_permissions(&path, changed).unwrap();
                }
                "extra" => fs::write(base.join("extra"), b"manual addition").unwrap(),
                "link" => {
                    fs::remove_file(&path).unwrap();
                    std::os::windows::fs::symlink_file(base.join("SKILL.md"), &path)
                        .expect("required Windows file reparse fixture privilege");
                }
                _ => unreachable!(),
            }
            let before = tree(&fixture.root);
            let result = verify_plan(&plan, VerificationLimits::default());
            assert_eq!(before, tree(&fixture.root));
            if scenario == "readonly" {
                fs::set_permissions(&path, original_permissions).unwrap();
            }
            let error = result.unwrap_err();
            assert!(
                matches!(
                    error.kind,
                    VerificationErrorKind::SourceChanged
                        | VerificationErrorKind::SourceInspection(_)
                        | VerificationErrorKind::DestinationChanged
                        | VerificationErrorKind::DestinationInspection(_)
                ),
                "{existing}/{scenario}: {error:?}"
            );
        }
    }
}
#[cfg(windows)]
#[test]
fn windows_competing_entries_invalidate_absence_without_touching_winners() {
    for scenario in ["file", "directory", "reparse"] {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.destination().parent().unwrap()).unwrap();
        let plan = fixture.plan();
        match scenario {
            "file" => fs::write(fixture.destination(), b"winner").unwrap(),
            "directory" => fs::create_dir(fixture.destination()).unwrap(),
            "reparse" => std::os::windows::fs::symlink_dir(&fixture.source, fixture.destination())
                .expect("required Windows directory reparse fixture privilege"),
            _ => unreachable!(),
        }
        let before = tree(&fixture.root);
        assert!(matches!(
            verify_plan(&plan, VerificationLimits::default())
                .unwrap_err()
                .kind,
            VerificationErrorKind::DestinationChanged | VerificationErrorKind::Namespace(_)
        ));
        assert_eq!(before, tree(&fixture.root));
    }
}
#[cfg(windows)]
#[test]
fn windows_read_only_verification_does_not_enable_copy_execution() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    assert!(verify_plan(&plan, VerificationLimits::default()).is_ok());
    let before = tree(&fixture.root);
    let report = apply_plan(&plan, ApplyOptions::default());
    assert_eq!(
        report.preflight_error.unwrap().kind,
        VerificationErrorKind::UnsupportedPlatform
    );
    assert!(
        report
            .outcomes
            .iter()
            .all(|outcome| matches!(outcome.status, OutcomeStatus::NotAttempted))
    );
    assert_eq!(before, tree(&fixture.root));
}

#[test]
fn committed_permission_failures_are_explicit_unsuccessful_and_receipt_compatible() {
    use agent_skills_kit::receipt::*;
    let fixture = Fixture::new();
    let plan = fixture.plan();
    let before = tree(&fixture.root);
    // Synthetic report projection only: this test does not perform a copy or
    // assert that the fixture's destination exists. Native finalization QA is separate.
    let failure = ApplyError {
        path: plan.destinations()[0].path().unwrap().to_owned(),
        kind: ApplyErrorKind::Io {
            operation: "synthetic finalization failure",
            kind: std::io::ErrorKind::PermissionDenied,
            raw_os_error: Some(5),
        },
    };
    let committed = OutcomeStatus::CommittedWithPermissionFailure(failure.clone());
    assert!(committed.payload_was_committed());
    assert!(OutcomeStatus::Created.payload_was_committed());
    for status in [
        OutcomeStatus::Unchanged,
        OutcomeStatus::NotAttempted,
        OutcomeStatus::Failed(failure),
    ] {
        assert!(!status.payload_was_committed());
    }
    let report = InstallReport {
        preflight_error: None,
        outcomes: vec![DestinationOutcome {
            path: plan.destinations()[0].path().map(Path::to_owned),
            attributions: plan.destinations()[0]
                .attributions()
                .iter()
                .map(|attribution| OutcomeAttribution {
                    source: plan.sources()[attribution.source_index]
                        .inventory()
                        .requested_root()
                        .to_owned(),
                    target_id: plan.targets()[attribution.target_index].id().to_owned(),
                })
                .collect(),
            status: committed,
        }],
        cleanup_errors: vec![],
    };
    assert!(!report.is_successful());
    let receipt = Receipt::from_report(
        &plan,
        &report,
        OperationId::new("synthetic-committed-permissions").unwrap(),
        ReceiptLimits::default(),
    )
    .unwrap();
    let destination = &receipt.document().destinations[0];
    assert_eq!(destination.status, ReceiptStatus::Failed);
    let diagnostic = destination.failure.as_ref().unwrap();
    assert_eq!(diagnostic.code, ReceiptFailureCode::Io);
    assert_eq!(diagnostic.os_error, Some(5));
    assert_eq!(
        diagnostic.operation.as_deref(),
        Some("payload committed; destination permission finalization failed")
    );
    assert_eq!(diagnostic.path.as_ref(), destination.path.as_ref());
    let encoded = receipt.to_json(ReceiptLimits::default()).unwrap();
    let parsed = Receipt::from_json(&encoded, ReceiptLimits::default()).unwrap();
    assert_eq!(parsed.document(), receipt.document());
    assert_eq!(parsed.document().schema_version, RECEIPT_SCHEMA_VERSION);
    assert_eq!(tree(&fixture.root), before);
    fixture.existing();
    let unchanged = fixture.plan();
    let before = tree(&fixture.root);
    assert!(matches!(
        Receipt::from_report(
            &unchanged,
            &report,
            OperationId::new("invalid-committed-unchanged").unwrap(),
            ReceiptLimits::default()
        ),
        Err(ReceiptError::InvalidDocument(_))
    ));
    assert_eq!(tree(&fixture.root), before);
}
