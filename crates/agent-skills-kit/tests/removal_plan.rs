use agent_skills_kit::{
    catalog::{Catalog, Scope},
    discovery::{DiscoveryLimits, SkillListing, list_skills},
    removal::{RemovalError, RemovalLimits, RemovalRequest, plan_removal},
    resolver::{InstallContext, native_platform, resolve},
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    context: InstallContext,
}
impl Fixture {
    fn new() -> Self {
        let mut created = None;
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "ask-removal-plan-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => {
                    created = Some(fs::canonicalize(path).unwrap());
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("owned fixture: {error}"),
            }
        }
        let root = created.expect("owned fixture collision budget");
        fs::create_dir(root.join("home")).unwrap();
        fs::create_dir(root.join("project")).unwrap();
        let mut context = InstallContext::new(native_platform().unwrap());
        context.user_home = Some(root.join("home"));
        context.project_root = Some(root.join("project"));
        Self { root, context }
    }
    fn skill(&self, target: &str, scope: Scope) -> PathBuf {
        let path = resolve(&Catalog::bundled().unwrap(), target, scope, &self.context)
            .unwrap()
            .preferred_write
            .path
            .join("sample");
        fs::create_dir_all(path.join("resources/empty")).unwrap();
        fs::write(path.join("SKILL.md"), b"---\nname: sample\ndescription: Original inert removal preview fixture.\n---\nNever execute.\n").unwrap();
        fs::write(path.join("resources/data.txt"), b"original data").unwrap();
        path
    }
    fn listing(&self, target: &str, scope: Scope) -> SkillListing {
        list_skills(
            &Catalog::bundled().unwrap(),
            target,
            scope,
            &self.context,
            DiscoveryLimits::default(),
        )
        .unwrap()
    }
    fn request(&self, listing: SkillListing) -> RemovalRequest {
        let location_index = listing
            .locations
            .iter()
            .position(|x| x.preferred_write)
            .unwrap();
        RemovalRequest {
            listing,
            location_index,
            skill_name: "sample".into(),
            context: self.context.clone(),
            limits: RemovalLimits::default(),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn complete_read_only_previews_include_all_payload_entries_root_and_known_readers() {
    for target in ["codex", "claude-code", "opencode"] {
        for scope in [Scope::User, Scope::Project] {
            let fixture = Fixture::new();
            let path = fixture.skill(target, scope);
            let before = fs::read(path.join("SKILL.md")).unwrap();
            let catalog = Catalog::bundled().unwrap();
            let plan =
                plan_removal(&catalog, fixture.request(fixture.listing(target, scope))).unwrap();
            assert_eq!(plan.path(), path);
            assert_eq!(plan.name(), "sample");
            assert_eq!(plan.scope(), scope);
            assert_eq!(plan.entries().len(), 4);
            assert_eq!(plan.metadata_bytes(), before);
            assert!(
                plan.readers()
                    .iter()
                    .any(|reader| reader.target_id == target
                        && reader.scope == scope
                        && reader.preferred_write)
            );
            if target == "codex" {
                assert!(
                    plan.readers()
                        .iter()
                        .any(|reader| reader.target_id == "opencode" && !reader.preferred_write)
                );
            }
            assert!(!plan.unresolved_readers().is_empty());
            assert!(plan.namespace_is_current().unwrap());
            assert_eq!(plan.confirmation_token().len(), 64);
            assert_eq!(fs::read(path.join("SKILL.md")).unwrap(), before);
            assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
            assert!(!fs::read_dir(path.parent().unwrap()).unwrap().any(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".ask")
            }));
            let approved = plan.approve_permanent_removal(&"0".repeat(64));
            assert!(matches!(approved, Err(RemovalError::ConfirmationMismatch)));
        }
    }
}
#[test]
fn operation_specific_fingerprint_is_deterministic_and_changes_with_selected_contents() {
    let fixture = Fixture::new();
    let path = fixture.skill("codex", Scope::User);
    let catalog = Catalog::bundled().unwrap();
    let plan = plan_removal(
        &catalog,
        fixture.request(fixture.listing("codex", Scope::User)),
    )
    .unwrap();
    let token = plan.confirmation_token().to_owned();
    drop(plan);
    let repeated = plan_removal(
        &catalog,
        fixture.request(fixture.listing("codex", Scope::User)),
    )
    .unwrap();
    assert_eq!(token, repeated.confirmation_token());
    drop(repeated);
    fs::write(path.join("resources/data.txt"), b"edited data").unwrap();
    let changed = plan_removal(
        &catalog,
        fixture.request(fixture.listing("codex", Scope::User)),
    )
    .unwrap();
    assert_ne!(token, changed.confirmation_token());
    let confirmation = changed.confirmation_token().to_owned();
    let approved = changed.approve_permanent_removal(&confirmation).unwrap();
    assert_eq!(approved.plan().confirmation_token(), confirmation);
    assert!(path.exists());
    drop(approved);
}
#[test]
fn stale_invalid_missing_and_wrong_context_selections_never_gain_removal_authority() {
    let fixture = Fixture::new();
    let path = fixture.skill("codex", Scope::User);
    let catalog = Catalog::bundled().unwrap();
    let listing = fixture.listing("codex", Scope::User);
    fs::write(path.join("resources/data.txt"), b"edited after listing").unwrap();
    assert!(matches!(
        plan_removal(&catalog, fixture.request(listing)),
        Err(RemovalError::Changed)
    ));
    let mut request = fixture.request(fixture.listing("codex", Scope::User));
    request.context.user_home = Some(fixture.root.join("project"));
    assert!(matches!(
        plan_removal(&catalog, request),
        Err(RemovalError::ContextMismatch)
    ));
    let mut request = fixture.request(fixture.listing("codex", Scope::User));
    request.skill_name = "../sample".into();
    assert!(matches!(
        plan_removal(&catalog, request),
        Err(RemovalError::InvalidSelection)
    ));
    let request = fixture.request(fixture.listing("claude-code", Scope::User));
    assert!(matches!(
        plan_removal(&catalog, request),
        Err(RemovalError::InvalidSelection)
    ));
    fs::write(path.join("SKILL.md"), b"invalid inert data").unwrap();
    assert!(matches!(
        plan_removal(
            &catalog,
            fixture.request(fixture.listing("codex", Scope::User))
        ),
        Err(RemovalError::Discovery(_))
    ));
    assert_eq!(
        fs::read(path.join("SKILL.md")).unwrap(),
        b"invalid inert data"
    );
}
#[test]
fn budgets_are_explicit_failures_without_staging_or_payload_changes() {
    let fixture = Fixture::new();
    let path = fixture.skill("codex", Scope::User);
    let catalog = Catalog::bundled().unwrap();
    for (targets, locations) in [(0, 1024), (1, 1024), (256, 0), (256, 1)] {
        let mut request = fixture.request(fixture.listing("codex", Scope::User));
        request.limits.max_targets = targets;
        request.limits.max_locations = locations;
        assert!(matches!(
            plan_removal(&catalog, request),
            Err(RemovalError::InvalidLimits | RemovalError::Limit(_))
        ));
        assert_eq!(
            fs::read(path.join("resources/data.txt")).unwrap(),
            b"original data"
        );
    }
}

#[test]
fn explicit_context_roots_equal_to_or_inside_the_payload_are_never_removal_targets() {
    let fixture = Fixture::new();
    let path = fixture.skill("codex", Scope::User);
    let catalog = Catalog::bundled().unwrap();
    for protected in [
        path.clone(),
        path.join("resources/empty"),
        path.join("future-project-root"),
    ] {
        for application in [false, true] {
            let mut request = fixture.request(fixture.listing("codex", Scope::User));
            if application {
                request.context.application_workspace = Some(protected.clone());
            } else {
                request.context.project_root = Some(protected.clone());
            }
            assert!(
                matches!(plan_removal(&catalog,request),Err(RemovalError::ProtectedRoot(root)) if root==protected)
            );
            assert_eq!(
                fs::read(path.join("resources/data.txt")).unwrap(),
                b"original data"
            );
        }
    }
}

// Original caller-defined metadata for root-safety tests, not new host evidence.
fn synthetic_container_catalog(id: &str, variable: &str) -> Catalog {
    use agent_skills_kit::catalog::{EvidenceRecord, PathBase};
    let mut document = Catalog::bundled().unwrap().document();
    let mut target = document
        .targets
        .iter()
        .find(|target| target.id == "codex")
        .unwrap()
        .clone();
    target.id = id.into();
    target.product_id = id.into();
    target.aliases.clear();
    target.variant = "fixture".into();
    target.scopes.retain(|scope| scope.scope == Scope::User);
    let scope = &mut target.scopes[0];
    scope.discovery.clear();
    let rule = scope.preferred_write.as_mut().unwrap();
    rule.path.base = PathBase::Environment {
        variable: variable.into(),
        fallback: None,
    };
    rule.path.components.clear();
    rule.evidence.source = Some(EvidenceRecord {
        url: "https://example.invalid/owned-root-safety-fixture".into(),
        section: "Original synthetic root safety test".into(),
        accessed: "2026-10-17".into(),
        applicability: "Synthetic caller metadata only; no real host path claim".into(),
    });
    rule.evidence.resolver_tests.clear();
    rule.evidence.discovery_tests.clear();
    rule.evidence.limitations.clear();
    document.targets.push(target);
    Catalog::new(document).unwrap()
}
#[test]
fn a_payload_cannot_be_a_resolved_host_container_or_contain_one() {
    let fixture = Fixture::new();
    let path = fixture.skill("codex", Scope::User);
    let catalog = synthetic_container_catalog("fixture-reader", "ASK_FIXTURE_READER_ROOT");
    for protected in [
        path.clone(),
        path.join("resources/empty"),
        path.join("future-container"),
    ] {
        let mut context = fixture.context.clone();
        context.overrides.insert(
            "ASK_FIXTURE_READER_ROOT".into(),
            protected.clone().into_os_string(),
        );
        let listing = list_skills(
            &catalog,
            "codex",
            Scope::User,
            &context,
            DiscoveryLimits::default(),
        )
        .unwrap();
        let mut request = fixture.request(listing);
        request.context = context;
        assert!(
            matches!(plan_removal(&catalog,request),Err(RemovalError::ProtectedRoot(root)) if root==protected)
        );
        assert!(path.join("SKILL.md").is_file());
    }
}
#[test]
fn an_override_does_not_authorize_deleting_the_explicit_user_home_itself() {
    let fixture = Fixture::new();
    let path = fixture.skill("codex", Scope::User);
    let catalog = synthetic_container_catalog("fixture-source", "ASK_FIXTURE_SOURCE_CONTAINER");
    let mut context = fixture.context.clone();
    context.user_home = Some(path.clone());
    context.overrides.insert(
        "ASK_FIXTURE_SOURCE_CONTAINER".into(),
        path.parent().unwrap().as_os_str().to_owned(),
    );
    let listing = list_skills(
        &catalog,
        "fixture-source",
        Scope::User,
        &context,
        DiscoveryLimits::default(),
    )
    .unwrap();
    let mut request = fixture.request(listing);
    request.context = context;
    assert!(
        matches!(plan_removal(&catalog,request),Err(RemovalError::ProtectedRoot(root)) if root==path)
    );
    assert_eq!(
        fs::read(path.join("resources/data.txt")).unwrap(),
        b"original data"
    );
}

#[cfg(unix)]
#[test]
fn protected_root_detection_uses_physical_ancestry_through_explicit_context_aliases() {
    let fixture = Fixture::new();
    let path = fixture.skill("codex", Scope::User);
    let catalog = Catalog::bundled().unwrap();
    let alias = fixture.root.join("explicit-parent-alias");
    std::os::unix::fs::symlink(&fixture.root, &alias).unwrap();
    let protected = alias.join("home/.agents/skills/sample/resources/empty");
    let mut request = fixture.request(fixture.listing("codex", Scope::User));
    request.context.project_root = Some(protected.clone());
    assert!(
        matches!(plan_removal(&catalog,request),Err(RemovalError::ProtectedRoot(root)) if root==protected)
    );
    assert_eq!(
        fs::read(path.join("resources/data.txt")).unwrap(),
        b"original data"
    );
}

#[test]
fn missing_exact_reader_roots_do_not_hide_unexpanded_ancestor_impact() {
    let fixture = Fixture::new();
    fixture.skill("codex", Scope::User);
    let catalog = Catalog::bundled().unwrap();
    let plan = plan_removal(
        &catalog,
        fixture.request(fixture.listing("codex", Scope::User)),
    )
    .unwrap();
    let project = fixture.context.project_root.as_ref().unwrap();
    let missing = project.join(".agents/skills");
    assert!(!missing.exists());
    assert!(
        plan.unresolved_readers()
            .iter()
            .any(|reader| reader.target_id == "opencode"
                && reader.scope == Scope::Project
                && reader.path.as_deref() == Some(missing.as_path())
                && reader.reason.contains("ancestor"))
    );
    assert_eq!(fs::read_dir(project).unwrap().count(), 0);
    drop(plan);
}
