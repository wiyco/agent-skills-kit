use agent_skills_kit::{
    catalog::{Catalog, Scope},
    discovery::{DiscoveryError, DiscoveryLimits, list_skills},
    resolver::{InstallContext, native_platform, resolve},
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "ask-discovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        fs::create_dir(root.join("home")).unwrap();
        fs::create_dir(root.join("project")).unwrap();
        Self(root)
    }
    fn context(&self) -> InstallContext {
        let mut context = InstallContext::new(native_platform().expect("native CI host"));
        context.user_home = Some(self.0.join("home"));
        context.project_root = Some(self.0.join("project"));
        context
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn skill(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    fs::create_dir_all(path.join("resources/empty")).unwrap();
    fs::write(path.join("SKILL.md"), format!("---\nname: {name}\ndescription: Original inert discovery fixture.\n---\nDo not execute this fixture.\n")).unwrap();
    fs::write(path.join("resources/original.txt"), b"original inert bytes").unwrap();
    path
}
fn tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(base: &Path, path: &Path, output: &mut Vec<(PathBuf, Vec<u8>)>) {
        let metadata = fs::symlink_metadata(path).unwrap();
        output.push((
            path.strip_prefix(base).unwrap().to_owned(),
            if metadata.is_file() {
                fs::read(path).unwrap()
            } else {
                Vec::new()
            },
        ));
        if metadata.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                walk(base, &entry.unwrap().path(), output);
            }
        }
    }
    let mut output = Vec::new();
    walk(root, root, &mut output);
    output.sort();
    output
}

#[test]
fn core_host_scopes_list_original_bytes_without_mutation() {
    for target in ["codex-cli", "claude-code", "opencode"] {
        for scope in [Scope::Project, Scope::User] {
            let fixture = Fixture::new();
            let catalog = Catalog::bundled().unwrap();
            let context = fixture.context();
            let resolved = resolve(&catalog, target, scope, &context).unwrap();
            let first = skill(&resolved.preferred_write.path, "first-skill");
            skill(&resolved.preferred_write.path, "second-skill");
            let before = tree(&fixture.0);
            let listing = list_skills(
                &catalog,
                target,
                scope,
                &context,
                DiscoveryLimits::default(),
            )
            .unwrap();
            assert_eq!(listing.target_id, resolved.target_id);
            let root = listing
                .locations
                .iter()
                .find(|root| root.preferred_write)
                .unwrap();
            assert!(root.problem.is_none(), "{:?}", root.problem);
            assert!(!root.missing);
            assert_eq!(root.skills.len(), 2);
            assert_eq!(root.skills[0].name, "first-skill");
            let inspected = root.skills[0].inspection.as_ref().unwrap();
            assert_eq!(inspected.metadata().name(), "first-skill");
            assert_eq!(
                inspected.metadata().source_bytes(),
                fs::read(first.join("SKILL.md")).unwrap()
            );
            assert_eq!(tree(&fixture.0), before);
            let second = list_skills(
                &catalog,
                target,
                scope,
                &context,
                DiscoveryLimits::default(),
            )
            .unwrap();
            assert_eq!(
                second
                    .locations
                    .iter()
                    .map(|x| (&x.path, x.skills.len()))
                    .collect::<Vec<_>>(),
                listing
                    .locations
                    .iter()
                    .map(|x| (&x.path, x.skills.len()))
                    .collect::<Vec<_>>()
            );
            // Windows intentionally retains no-delete namespace handles: close
            // observations before the fixture owner tears down its own tree.
            drop(second);
            drop(listing);
        }
    }
}

#[test]
fn missing_roots_remain_absent_and_reader_roles_are_not_write_permission() {
    let fixture = Fixture::new();
    let catalog = Catalog::bundled().unwrap();
    let context = fixture.context();
    let before = tree(&fixture.0);
    let listing = list_skills(
        &catalog,
        "opencode",
        Scope::Project,
        &context,
        DiscoveryLimits::default(),
    )
    .unwrap();
    assert!(listing.locations.len() > 1);
    assert_eq!(
        listing
            .locations
            .iter()
            .filter(|x| x.preferred_write)
            .count(),
        1
    );
    assert!(
        listing
            .locations
            .iter()
            .all(|root| root.missing && root.problem.is_none() && root.skills.is_empty())
    );
    assert!(!listing.unexpanded_ancestors.is_empty());
    assert_eq!(tree(&fixture.0), before);
    drop(listing);
}

#[test]
fn malformed_and_non_directory_entries_remain_visible_instead_of_disappearing() {
    let fixture = Fixture::new();
    let catalog = Catalog::bundled().unwrap();
    let context = fixture.context();
    let root = resolve(&catalog, "codex", Scope::User, &context)
        .unwrap()
        .preferred_write
        .path;
    fs::create_dir_all(root.join("broken-skill")).unwrap();
    fs::write(root.join("broken-skill/SKILL.md"), b"not frontmatter").unwrap();
    fs::write(root.join("not-a-skill.txt"), b"original unrelated data").unwrap();
    skill(&root, "valid-skill");
    let before = tree(&fixture.0);
    let listing = list_skills(
        &catalog,
        "codex",
        Scope::User,
        &context,
        DiscoveryLimits::default(),
    )
    .unwrap();
    let location = listing
        .locations
        .iter()
        .find(|x| x.preferred_write)
        .unwrap();
    assert!(location.problem.is_none(), "{:?}", location.problem);
    assert_eq!(location.skills.len(), 3);
    assert!(location.skills[0].inspection.is_err());
    assert!(location.skills[1].inspection.is_err());
    assert!(
        location.skills[2].inspection.is_ok(),
        "{:?}",
        location.skills[2].inspection
    );
    assert_eq!(tree(&fixture.0), before);
    drop(listing);
}

#[test]
fn whole_listing_and_inventory_budgets_never_turn_partial_work_into_empty_success() {
    let fixture = Fixture::new();
    let catalog = Catalog::bundled().unwrap();
    let context = fixture.context();
    let root = resolve(&catalog, "codex", Scope::User, &context)
        .unwrap()
        .preferred_write
        .path;
    skill(&root, "first-skill");
    skill(&root, "second-skill");
    let before = tree(&fixture.0);
    let limits = DiscoveryLimits {
        max_candidates: 1,
        ..DiscoveryLimits::default()
    };
    let listing = list_skills(&catalog, "codex", Scope::User, &context, limits).unwrap();
    assert!(matches!(
        listing.locations[0].problem,
        Some(DiscoveryError::Limit("candidates"))
    ));
    drop(listing);
    let limits = DiscoveryLimits {
        max_read_bytes: 1,
        ..DiscoveryLimits::default()
    };
    let listing = list_skills(&catalog, "codex", Scope::User, &context, limits).unwrap();
    assert!(
        listing.locations[0]
            .skills
            .iter()
            .all(|x| x.inspection.is_err())
    );
    drop(listing);
    assert!(matches!(
        list_skills(
            &catalog,
            "codex",
            Scope::User,
            &context,
            DiscoveryLimits {
                max_candidates: 0,
                ..DiscoveryLimits::default()
            }
        ),
        Err(DiscoveryError::InvalidLimits)
    ));
    assert!(matches!(
        list_skills(
            &catalog,
            "unknown",
            Scope::User,
            &context,
            DiscoveryLimits::default()
        ),
        Err(DiscoveryError::Resolution(_))
    ));
    assert!(matches!(
        list_skills(
            &catalog,
            "opencode",
            Scope::User,
            &context,
            DiscoveryLimits {
                max_locations: 1,
                ..DiscoveryLimits::default()
            }
        ),
        Err(DiscoveryError::Limit("locations"))
    ));
    assert_eq!(tree(&fixture.0), before);
}

#[cfg(any(unix, windows))]
fn directory_link(source: &Path, destination: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(source, destination).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(source, destination)
        .expect("native reparse fixture requires Windows symlink privileges");
}

#[test]
fn host_components_and_skill_links_never_follow_outside_the_explicit_context() {
    let fixture = Fixture::new();
    let catalog = Catalog::bundled().unwrap();
    let context = fixture.context();
    let outside = fixture.0.join("outside");
    fs::create_dir(&outside).unwrap();
    skill(&outside.join("skills"), "outside-skill");
    directory_link(&outside, &fixture.0.join("home/.agents"));
    let listing = list_skills(
        &catalog,
        "codex",
        Scope::User,
        &context,
        DiscoveryLimits::default(),
    )
    .unwrap();
    assert!(
        listing
            .locations
            .iter()
            .all(|x| x.problem.is_some() && x.skills.is_empty())
    );
    drop(listing);
    fs::remove_dir(fixture.0.join("home/.agents"))
        .or_else(|_| fs::remove_file(fixture.0.join("home/.agents")))
        .unwrap();
    let root = fixture.0.join("home/.agents/skills");
    fs::create_dir_all(&root).unwrap();
    directory_link(
        &outside.join("skills/outside-skill"),
        &root.join("linked-skill"),
    );
    let before = tree(&fixture.0);
    let listing = list_skills(
        &catalog,
        "codex",
        Scope::User,
        &context,
        DiscoveryLimits::default(),
    )
    .unwrap();
    let root = listing
        .locations
        .iter()
        .find(|x| x.preferred_write)
        .unwrap();
    assert_eq!(root.skills.len(), 1);
    assert!(root.skills[0].inspection.is_err());
    assert_eq!(tree(&fixture.0), before);
    drop(listing);
}

#[cfg(unix)]
#[test]
fn unreadable_container_is_an_error_not_a_missing_or_empty_location() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let catalog = Catalog::bundled().unwrap();
    let context = fixture.context();
    let root = resolve(&catalog, "codex", Scope::User, &context)
        .unwrap()
        .preferred_write
        .path;
    skill(&root, "original-skill");
    struct Restore(PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
    let guard = Restore(root.clone());
    fs::set_permissions(&root, fs::Permissions::from_mode(0o000)).unwrap();
    let listing = list_skills(
        &catalog,
        "codex",
        Scope::User,
        &context,
        DiscoveryLimits::default(),
    )
    .unwrap();
    assert!(
        listing
            .locations
            .iter()
            .all(|x| !x.missing && x.problem.is_some() && x.skills.is_empty()),
        "fixture needs an unprivileged native test account"
    );
    drop(listing);
    drop(guard);
}

#[cfg(windows)]
#[test]
fn sharing_denied_container_is_an_error_not_a_missing_or_empty_location() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };
    let fixture = Fixture::new();
    let catalog = Catalog::bundled().unwrap();
    let context = fixture.context();
    let root = resolve(&catalog, "codex", Scope::User, &context)
        .unwrap()
        .preferred_write
        .path;
    skill(&root, "original-skill");
    let guard = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&root)
        .unwrap();
    let listing = list_skills(
        &catalog,
        "codex",
        Scope::User,
        &context,
        DiscoveryLimits::default(),
    )
    .unwrap();
    assert!(
        listing
            .locations
            .iter()
            .all(|x| !x.missing && x.problem.is_some() && x.skills.is_empty())
    );
    drop(listing);
    drop(guard);
}

#[cfg(target_os = "linux")]
#[test]
fn native_case_collisions_are_visible_and_neither_entry_becomes_selectable() {
    let fixture = Fixture::new();
    let catalog = Catalog::bundled().unwrap();
    let context = fixture.context();
    let root = resolve(&catalog, "codex", Scope::User, &context)
        .unwrap()
        .preferred_write
        .path;
    skill(&root, "original-skill");
    fs::create_dir(root.join("ORIGINAL-SKILL")).unwrap();
    let listing = list_skills(
        &catalog,
        "codex",
        Scope::User,
        &context,
        DiscoveryLimits::default(),
    )
    .unwrap();
    let root = listing
        .locations
        .iter()
        .find(|x| x.preferred_write)
        .unwrap();
    assert_eq!(root.skills.len(), 2);
    assert!(
        root.skills
            .iter()
            .all(|x| matches!(x.inspection, Err(DiscoveryError::AmbiguousName)))
    );
    drop(listing);
}

#[test]
fn opaque_native_names_are_retained_or_explicitly_refused_by_the_filesystem() {
    use std::ffi::OsString;
    #[cfg(unix)]
    let name = {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![b'a', 0xff])
    };
    #[cfg(windows)]
    let name = {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[u16::from(b'a'), 0xd800])
    };
    let fixture = Fixture::new();
    let catalog = Catalog::bundled().unwrap();
    let context = fixture.context();
    let root = resolve(&catalog, "codex", Scope::User, &context)
        .unwrap()
        .preferred_write
        .path;
    if let Err(error) = fs::create_dir_all(root.join(&name)) {
        #[cfg(target_os = "macos")]
        {
            assert_eq!(
                error.raw_os_error(),
                Some(libc::EILSEQ),
                "not an unrelated permission/sandbox failure"
            );
            return; // Actual APFS name-creation refusal, not opaque-name discovery evidence.
        }
        #[cfg(not(target_os = "macos"))]
        panic!("native opaque candidate fixture must be creatable: {error}");
    }
    let listing = list_skills(
        &catalog,
        "codex",
        Scope::User,
        &context,
        DiscoveryLimits::default(),
    )
    .unwrap();
    let root = listing
        .locations
        .iter()
        .find(|x| x.preferred_write)
        .unwrap();
    assert_eq!(root.skills.len(), 1);
    assert_eq!(root.skills[0].name, name);
    assert!(
        matches!(&root.skills[0].inspection, Err(DiscoveryError::Inventory(error)) if error.kind == agent_skills_kit::validation::InventoryErrorKind::NonUtf8SkillName)
    );
    drop(listing);
}

#[test]
fn a_selected_snapshot_cannot_silently_adopt_edits_when_copy_is_planned() {
    use agent_skills_kit::planner::{InstallRequest, PlanLimits, plan_install};
    let fixture = Fixture::new();
    let catalog = Catalog::bundled().unwrap();
    let context = fixture.context();
    let root = resolve(&catalog, "codex", Scope::User, &context)
        .unwrap()
        .preferred_write
        .path;
    let source = skill(&root, "original-skill");
    let listing = list_skills(
        &catalog,
        "codex",
        Scope::User,
        &context,
        DiscoveryLimits::default(),
    )
    .unwrap();
    let selected = listing
        .locations
        .iter()
        .flat_map(|location| &location.skills)
        .find_map(|row| row.inspection.as_ref().ok())
        .unwrap()
        .as_ref()
        .clone();
    fs::write(
        source.join("resources/original.txt"),
        b"externally changed inert bytes",
    )
    .unwrap();
    let skills = [selected];
    let request = InstallRequest {
        skills: &skills,
        target_ids: &["claude-code"],
        scope: Scope::User,
        context: &context,
        limits: PlanLimits::default(),
    };
    assert!(plan_install(&catalog, request).is_err());
    assert!(!fixture.0.join("home/.claude").exists());
    assert_eq!(
        fs::read(source.join("resources/original.txt")).unwrap(),
        b"externally changed inert bytes"
    );
    drop(skills);
    drop(listing);
}
