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
    native_writes_remain_distinct("gemini-cli");
}
#[test]
fn cursor_ide_native_writes_are_not_shared_reader_rewrites() {
    native_writes_remain_distinct("cursor-ide");
}
fn native_writes_remain_distinct(target: &str) {
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
                    target_ids: &[target, "codex-cli", target],
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
        let selected = resolve(&catalog, target, scope, &context).unwrap();
        assert!(
            selected
                .discovery
                .iter()
                .any(|reader| reader.location.path == codex.preferred_write.path)
        );
        assert_ne!(selected.preferred_write.path, codex.preferred_write.path);
        let report = apply_plan(&plan, ApplyOptions::default());
        assert!(report.is_successful(), "{report:?}");
        assert!(
            report
                .outcomes
                .iter()
                .all(|row| row.status == OutcomeStatus::Created)
        );
        for destination in [selected.preferred_write.path, codex.preferred_write.path] {
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

#[test]
fn cursor_ide_scopes_and_variant_boundaries() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = context();
    for variable in ["CURSOR_HOME", "CLAUDE_CONFIG_DIR", "CODEX_HOME"] {
        context
            .overrides
            .insert(variable.into(), "unverified override".into());
    }
    let target = catalog.get("cursor-ide").unwrap();
    assert_eq!(target.product_id, "cursor");
    assert_eq!(target.variant, "ide");
    for (scope, root) in [
        (Scope::User, context.user_home.as_ref().unwrap()),
        (Scope::Project, context.project_root.as_ref().unwrap()),
    ] {
        let resolved = resolve(&catalog, "cursor-ide", scope, &context).unwrap();
        assert_eq!(resolved.preferred_write.path, root.join(".cursor/skills"));
        assert_eq!(resolved.discovery.len(), 4);
        for (reader, folder) in resolved
            .discovery
            .iter()
            .zip([".agents", ".cursor", ".claude", ".codex"])
        {
            assert_eq!(reader.location.path, root.join(folder).join("skills"));
            assert_eq!(reader.traversal, DiscoveryTraversal::Exact);
            assert_eq!(
                reader.location.evidence.source.as_ref().unwrap().url,
                "https://cursor.com/docs/skills"
            );
            assert!(reader.location.evidence.discovery_tests.is_empty());
        }
        assert!(
            resolved
                .warnings
                .contains(&ResolutionWarning::HostDiscoveryUntested)
        );
        assert!(
            resolved
                .warnings
                .contains(&ResolutionWarning::PlacementNotExclusive)
        );
        assert!(
            !resolved
                .warnings
                .contains(&ResolutionWarning::ResolverEvidenceAbsent)
        );
    }
    for id in ["cursor", "cursor-cli", "cursor-cloud"] {
        assert!(catalog.get(id).is_none());
    }
    assert!(matches!(
        resolve(
            &catalog,
            "cursor-ide",
            Scope::ApplicationWorkspace,
            &context
        ),
        Err(ResolveError::UnsupportedScope { .. })
    ));
    context.user_home = None;
    assert!(matches!(
        resolve(&catalog, "cursor-ide", Scope::User, &context),
        Err(ResolveError::MissingContext(ContextBase::Home))
    ));
    context.project_root = None;
    assert!(matches!(
        resolve(&catalog, "cursor-ide", Scope::Project, &context),
        Err(ResolveError::MissingContext(ContextBase::Project))
    ));
}

#[test]
fn copilot_variants_keep_independent_readers_and_context() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = context();
    for variable in ["COPILOT_HOME", "CLAUDE_CONFIG_DIR", "XDG_CONFIG_HOME"] {
        context
            .overrides
            .insert(variable.into(), "unverified override".into());
    }
    for variant in ["cli", "vscode"] {
        let id = format!("github-copilot-{variant}");
        let target = catalog.get(&id).unwrap();
        assert_eq!(target.product_id, "github-copilot");
        assert_eq!(target.variant, variant);
        for (scope, root, native) in [
            (Scope::User, context.user_home.as_ref().unwrap(), ".copilot"),
            (
                Scope::Project,
                context.project_root.as_ref().unwrap(),
                ".github",
            ),
        ] {
            let resolved = resolve(&catalog, &id, scope, &context).unwrap();
            assert_eq!(
                resolved.preferred_write.path,
                root.join(native).join("skills")
            );
            let folders = if scope == Scope::Project {
                vec![".github", ".claude", ".agents"]
            } else if variant == "cli" {
                vec![".copilot", ".agents"]
            } else {
                vec![".copilot", ".claude", ".agents"]
            };
            assert_eq!(resolved.discovery.len(), folders.len());
            for (reader, folder) in resolved.discovery.iter().zip(folders) {
                assert_eq!(reader.location.path, root.join(folder).join("skills"));
                assert_eq!(reader.traversal, DiscoveryTraversal::Exact);
                assert!(reader.location.evidence.discovery_tests.is_empty());
                assert_eq!(
                    reader.location.evidence.source,
                    resolved.preferred_write.evidence.source
                );
            }
            let expected = if variant == "cli" {
                "https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-skills"
            } else {
                "https://code.visualstudio.com/docs/agent-customization/agent-skills"
            };
            assert_eq!(
                resolved
                    .preferred_write
                    .evidence
                    .source
                    .as_ref()
                    .unwrap()
                    .url,
                expected
            );
            assert!(
                resolved
                    .warnings
                    .contains(&ResolutionWarning::HostDiscoveryUntested)
            );
            assert!(
                resolved
                    .warnings
                    .contains(&ResolutionWarning::PlacementNotExclusive)
            );
            assert!(
                !resolved
                    .warnings
                    .contains(&ResolutionWarning::ResolverEvidenceAbsent)
            );
        }
        assert!(matches!(
            resolve(&catalog, &id, Scope::ApplicationWorkspace, &context),
            Err(ResolveError::UnsupportedScope { .. })
        ));
        let empty = InstallContext::new(native_platform().unwrap());
        assert!(matches!(
            resolve(&catalog, &id, Scope::User, &empty),
            Err(ResolveError::MissingContext(ContextBase::Home))
        ));
        assert!(matches!(
            resolve(&catalog, &id, Scope::Project, &empty),
            Err(ResolveError::MissingContext(ContextBase::Project))
        ));
    }
    for id in ["copilot", "github-copilot", "copilot-cloud", "copilot-app"] {
        assert!(catalog.get(id).is_none());
    }
}

#[test]
fn copilot_coincident_writes_preserve_both_variant_attributions() {
    for scope in [Scope::User, Scope::Project] {
        let fixture = Fixture::new();
        let source = fixture.0.join("source/sample");
        fs::create_dir_all(source.join("empty")).unwrap();
        fs::write(source.join("SKILL.md"), b"---\nname: sample\ndescription: Original inert Copilot variant fixture\n---\nData only.\n").unwrap();
        fs::write(
            source.join("resource café.txt"),
            b"unchanged original bytes",
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
                    target_ids: &[
                        "github-copilot-cli",
                        "github-copilot-vscode",
                        "github-copilot-cli",
                    ],
                    scope,
                    context: &context,
                    limits: PlanLimits::default(),
                },
            )
            .unwrap()
        };
        let plan = preview();
        assert!(plan.is_applicable());
        assert_eq!(plan.targets().len(), 2);
        assert_eq!(plan.destinations().len(), 1);
        assert_eq!(
            plan.destinations()[0]
                .attributions()
                .iter()
                .map(|attribution| {
                    assert_eq!(attribution.source_index, 0);
                    plan.targets()[attribution.target_index].id()
                })
                .collect::<Vec<_>>(),
            ["github-copilot-cli", "github-copilot-vscode"]
        );
        assert_eq!(
            plan.intended_writes()
                .iter()
                .filter(|write| write.kind == IntendedWriteKind::CopyFile)
                .count(),
            2
        );
        let destination = plan.destinations()[0].path().unwrap().to_path_buf();
        assert!(!destination.exists());
        let report = apply_plan(&plan, ApplyOptions::default());
        assert!(report.is_successful(), "{report:?}");
        assert_eq!(report.outcomes.len(), 1);
        assert_eq!(report.outcomes[0].status, OutcomeStatus::Created);
        for name in ["SKILL.md", "resource café.txt"] {
            assert_eq!(
                fs::read(destination.join(name)).unwrap(),
                fs::read(source.join(name)).unwrap()
            );
        }
        assert!(destination.join("empty").is_dir());
        let repeated = apply_plan(&preview(), ApplyOptions::default());
        assert!(repeated.is_successful(), "{repeated:?}");
        assert_eq!(repeated.outcomes[0].status, OutcomeStatus::Unchanged);
        fs::write(
            destination.join("resource café.txt"),
            b"manual edit must survive approval",
        )
        .unwrap();
        let conflict = preview();
        assert!(!conflict.is_applicable());
        assert!(!apply_plan(&conflict, ApplyOptions::default()).is_successful());
        assert_eq!(
            fs::read(destination.join("resource café.txt")).unwrap(),
            b"manual edit must survive approval"
        );
    }
}
