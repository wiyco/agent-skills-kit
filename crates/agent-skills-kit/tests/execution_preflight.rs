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
