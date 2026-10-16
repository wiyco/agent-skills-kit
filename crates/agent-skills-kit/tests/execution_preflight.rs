#![cfg(any(target_os = "linux", target_os = "macos"))]

use agent_skills_kit::{catalog::*, executor::*, planner::*, resolver::*, validation::*};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
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
            use std::os::unix::ffi::OsStrExt;
            fs::read_link(path).unwrap().as_os_str().as_bytes().to_vec()
        } else {
            Vec::new()
        };
        output.push((
            path.strip_prefix(root).unwrap().to_owned(),
            bytes,
            metadata.mode(),
        ));
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
    fs::set_permissions(
        fixture.source.join("resource"),
        fs::Permissions::from_mode(0o750),
    )
    .unwrap();
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
