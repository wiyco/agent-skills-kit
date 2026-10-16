//! Native adapter evidence, separate from executing or discovering any real host.
use agent_skills_kit::{
    catalog::*,
    executor::{ApplyOptions, OutcomeStatus, apply_plan},
    planner::*,
    resolver::*,
    validation::{InventoryLimits, inventory_skill},
};
use std::{
    ffi::OsString,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
fn context() -> InstallContext {
    let mut context = InstallContext::new(native_platform().unwrap());
    let root = std::env::temp_dir().join("ask-expanded-inert");
    context.user_home = Some(root.join("home space"));
    context.project_root = Some(root.join("project 日本語"));
    context
}
#[test]
fn gemini_explicit_scopes_read_aliases_and_configuration_gaps() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = context();
    context.overrides.insert(
        OsString::from("GEMINI_CLI_HOME"),
        OsString::from("unverified override must be ignored"),
    );
    for (scope, root) in [
        (Scope::User, context.user_home.as_ref().unwrap()),
        (Scope::Project, context.project_root.as_ref().unwrap()),
    ] {
        let result = resolve(&catalog, "gemini-cli", scope, &context).unwrap();
        assert_eq!(result.preferred_write.path, root.join(".gemini/skills"));
        assert_eq!(result.discovery.len(), 2);
        assert_eq!(
            result.discovery[0].location.path,
            root.join(".agents/skills")
        );
        assert_eq!(result.discovery[1].location, result.preferred_write);
        assert!(
            result
                .discovery
                .iter()
                .all(|location| location.traversal == DiscoveryTraversal::Exact)
        );
        assert!(
            result
                .warnings
                .contains(&ResolutionWarning::PlacementNotExclusive)
        );
        assert!(
            result
                .warnings
                .contains(&ResolutionWarning::HostDiscoveryUntested)
        );
        assert!(
            !result
                .warnings
                .contains(&ResolutionWarning::ResolverEvidenceAbsent)
        );
        assert_eq!(
            result.preferred_write.evidence.source.as_ref().unwrap().url,
            "https://geminicli.com/docs/cli/skills/"
        );
        assert!(result.preferred_write.evidence.discovery_tests.is_empty());
    }
    assert!(catalog.get("gemini").is_none());
    assert!(catalog.get("gemini-code-assist").is_none());
    assert!(matches!(
        resolve(
            &catalog,
            "gemini-cli",
            Scope::ApplicationWorkspace,
            &context
        ),
        Err(ResolveError::UnsupportedScope { .. })
    ));
    context.user_home = None;
    assert!(matches!(
        resolve(&catalog, "gemini-cli", Scope::User, &context),
        Err(ResolveError::MissingContext(ContextBase::Home))
    ));
    context.project_root = None;
    assert!(matches!(
        resolve(&catalog, "gemini-cli", Scope::Project, &context),
        Err(ResolveError::MissingContext(ContextBase::Project))
    ));
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "ask-expanded-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("owned expanded fixture: {error}"),
            }
        }
        panic!("expanded fixture collision budget");
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn gemini_reader_alias_does_not_rewrite_native_installation() {
    for scope in [Scope::User, Scope::Project] {
        let fixture = Fixture::new();
        let source = fixture.0.join("source/sample");
        fs::create_dir_all(source.join("empty")).unwrap();
        fs::write(source.join("SKILL.md"),b"---\nname: sample\ndescription: Original inert expanded-target fixture\n---\nData only.\n").unwrap();
        fs::write(
            source.join("reference café.txt"),
            b"original resource bytes",
        )
        .unwrap();
        let mut context = InstallContext::new(native_platform().unwrap());
        context.user_home = Some(fixture.0.join("home"));
        context.project_root = Some(fixture.0.join("project"));
        fs::create_dir(context.user_home.as_ref().unwrap()).unwrap();
        fs::create_dir(context.project_root.as_ref().unwrap()).unwrap();
        let sources = [inventory_skill(&source, InventoryLimits::default()).unwrap()];
        let catalog = Catalog::bundled().unwrap();
        let preview = || {
            plan_install(
                &catalog,
                InstallRequest {
                    skills: &sources,
                    target_ids: &["gemini-cli", "codex-cli", "gemini-cli"],
                    scope,
                    context: &context,
                    limits: PlanLimits::default(),
                },
            )
            .unwrap()
        };
        let plan = preview();
        assert!(plan.is_applicable());
        assert_eq!(plan.destinations().len(), 2);
        assert!(
            plan.destinations()
                .iter()
                .all(|row| !row.path().unwrap().exists())
        );
        let codex = resolve(&catalog, "codex", scope, &context).unwrap();
        let gemini = resolve(&catalog, "gemini-cli", scope, &context).unwrap();
        assert_eq!(
            gemini.discovery[0].location.path,
            codex.preferred_write.path
        );
        assert_ne!(gemini.preferred_write.path, codex.preferred_write.path);
        let report = apply_plan(&plan, ApplyOptions::default());
        assert!(report.is_successful(), "{report:?}");
        assert!(
            report
                .outcomes
                .iter()
                .all(|row| row.status == OutcomeStatus::Created)
        );
        for destination in [gemini.preferred_write.path, codex.preferred_write.path] {
            assert_eq!(
                fs::read(destination.join("sample/SKILL.md")).unwrap(),
                fs::read(source.join("SKILL.md")).unwrap()
            );
        }
        let repeated = apply_plan(&preview(), ApplyOptions::default());
        assert!(repeated.is_successful(), "{repeated:?}");
        assert!(
            repeated
                .outcomes
                .iter()
                .all(|row| row.status == OutcomeStatus::Unchanged)
        );
    }
}
