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
    native_writes_with_override(target, None);
}
fn native_writes_with_override(target: &str, native_override: Option<&str>) {
    native_writes_with_expected_overlap(target, native_override, |_| true);
}
fn native_writes_with_expected_overlap(
    target: &str,
    native_override: Option<&str>,
    shared_reader: fn(Scope) -> bool,
) {
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
        if let Some(variable) = native_override {
            let alternate = fixture.0.join("alternate native root 日本語");
            fs::create_dir(&alternate).unwrap();
            context
                .overrides
                .insert(variable.into(), alternate.into_os_string());
        }
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
        assert_eq!(
            selected
                .discovery
                .iter()
                .any(|reader| reader.location.path == codex.preferred_write.path),
            shared_reader(scope)
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
    coincident_writes_preserve_variants(["github-copilot-cli", "github-copilot-vscode"], false);
}
#[test]
fn cline_coincident_writes_preserve_both_variant_attributions() {
    coincident_writes_preserve_variants(["cline-cli", "cline-vscode"], false);
    coincident_writes_preserve_variants(["cline-cli", "cline-vscode"], true);
}
fn coincident_writes_preserve_variants(targets: [&str; 2], cline_override: bool) {
    for scope in [Scope::User, Scope::Project] {
        let fixture = Fixture::new();
        let source = fixture.0.join("source/sample");
        fs::create_dir_all(source.join("empty")).unwrap();
        fs::write(source.join("SKILL.md"), b"---\nname: sample\ndescription: Original inert coincident-target fixture\n---\nData only.\n").unwrap();
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
        if cline_override {
            let alternate = fixture.0.join("alternate config 日本語");
            fs::create_dir(&alternate).unwrap();
            context
                .overrides
                .insert("CLINE_DIR".into(), alternate.into_os_string());
        }
        let sources = [inventory_skill(&source, InventoryLimits::default()).unwrap()];
        let catalog = Catalog::bundled().unwrap();
        let preview = || {
            plan_install(
                &catalog,
                InstallRequest {
                    skills: &sources,
                    target_ids: &[targets[0], targets[1], targets[0]],
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
            targets
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

#[test]
fn cline_scopes_overrides_and_variant_boundaries() {
    let catalog = Catalog::bundled().unwrap();
    for variant in ["cli", "vscode"] {
        let id = format!("cline-{variant}");
        let mut context = context();
        context
            .overrides
            .insert("CLINE_DATA_DIR".into(), "unrelated data root".into());
        context
            .overrides
            .insert("CLAUDE_CONFIG_DIR".into(), "unrelated Claude root".into());
        assert_eq!(catalog.get(&id).unwrap().variant, variant);
        assert_eq!(catalog.get(&id).unwrap().product_id, "cline");
        for (scope, root) in [
            (Scope::User, context.user_home.as_ref().unwrap()),
            (Scope::Project, context.project_root.as_ref().unwrap()),
        ] {
            let resolved = resolve(&catalog, &id, scope, &context).unwrap();
            assert_eq!(resolved.preferred_write.path, root.join(".cline/skills"));
            let folders = if scope == Scope::User {
                vec![".cline", ".agents"]
            } else if variant == "cli" {
                vec![".cline", ".clinerules", ".agents"]
            } else {
                vec![".cline", ".clinerules", ".agents", ".claude"]
            };
            assert_eq!(resolved.discovery.len(), folders.len());
            for (reader, folder) in resolved.discovery.iter().zip(folders) {
                assert_eq!(reader.location.path, root.join(folder).join("skills"));
                assert_eq!(reader.traversal, DiscoveryTraversal::Exact);
                assert!(reader.location.evidence.discovery_tests.is_empty());
                assert_eq!(
                    reader.location.evidence.source.as_ref().unwrap().url,
                    if folder == ".claude" {
                        "https://docs.cline.bot/customization/skills"
                    } else {
                        "https://docs.cline.bot/getting-started/config"
                    }
                );
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
        let project = resolve(&catalog, &id, Scope::Project, &context).unwrap();
        let alternate = context
            .user_home
            .as_ref()
            .unwrap()
            .join("alternate config 日本語");
        context
            .overrides
            .insert("CLINE_DIR".into(), alternate.clone().into_os_string());
        let user = resolve(&catalog, &id, Scope::User, &context).unwrap();
        assert_eq!(user.preferred_write.path, alternate.join("skills"));
        assert_eq!(
            user.preferred_write.origin,
            PathOrigin::Override("CLINE_DIR".into())
        );
        assert_eq!(user.discovery[0].location, user.preferred_write);
        assert_eq!(
            user.discovery[1].location.path,
            context.user_home.as_ref().unwrap().join(".agents/skills")
        );
        assert_eq!(
            project,
            resolve(&catalog, &id, Scope::Project, &context).unwrap()
        );
        for value in ["", "relative", "~/.cline"] {
            context.overrides.insert("CLINE_DIR".into(), value.into());
            assert!(matches!(
                resolve(&catalog, &id, Scope::User, &context),
                Err(ResolveError::InvalidOverride { .. })
            ));
        }
        context.overrides.remove(&OsString::from("CLINE_DIR"));
        assert!(matches!(
            resolve(&catalog, &id, Scope::ApplicationWorkspace, &context),
            Err(ResolveError::UnsupportedScope { .. })
        ));
        context.user_home = None;
        assert!(matches!(
            resolve(&catalog, &id, Scope::User, &context),
            Err(ResolveError::MissingContext(ContextBase::Home))
        ));
        context.project_root = None;
        assert!(matches!(
            resolve(&catalog, &id, Scope::Project, &context),
            Err(ResolveError::MissingContext(ContextBase::Project))
        ));
    }
    for id in [
        "cline",
        "cline-jetbrains",
        "cline-desktop",
        "cline-cloud",
        "cline-sdk",
    ] {
        assert!(catalog.get(id).is_none());
    }
}

#[test]
fn roo_reader_alias_does_not_rewrite_native_installation() {
    native_writes_remain_distinct("roo-code-vscode");
}
#[test]
fn roo_vscode_scopes_and_fork_boundaries() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = context();
    for variable in [
        "ROO_CODE_HOME",
        "CLINE_DIR",
        "CLAUDE_CONFIG_DIR",
        "KILO_HOME",
    ] {
        context
            .overrides
            .insert(variable.into(), "unverified relocation".into());
    }
    let target = catalog.get("roo-code-vscode").unwrap();
    assert_eq!(target.product_id, "roo-code");
    assert_eq!(target.variant, "vscode");
    for (scope, root) in [
        (Scope::User, context.user_home.as_ref().unwrap()),
        (Scope::Project, context.project_root.as_ref().unwrap()),
    ] {
        let result = resolve(&catalog, "roo-code-vscode", scope, &context).unwrap();
        assert_eq!(result.preferred_write.path, root.join(".roo/skills"));
        assert_eq!(result.discovery.len(), 2);
        assert_eq!(result.discovery[0].location, result.preferred_write);
        assert_eq!(
            result.discovery[1].location.path,
            root.join(".agents/skills")
        );
        for reader in &result.discovery {
            assert_eq!(reader.traversal, DiscoveryTraversal::Exact);
            assert_eq!(
                reader.location.evidence.source.as_ref().unwrap().url,
                "https://roocodeinc.github.io/Roo-Code/features/skills/"
            );
            assert!(reader.location.evidence.discovery_tests.is_empty());
        }
        assert!(
            result
                .warnings
                .contains(&ResolutionWarning::HostDiscoveryUntested)
        );
        assert!(
            result
                .warnings
                .contains(&ResolutionWarning::PlacementNotExclusive)
        );
        assert!(
            !result
                .warnings
                .contains(&ResolutionWarning::ResolverEvidenceAbsent)
        );
    }
    for id in ["roo", "roo-code", "roo-code-cli", "roo-code-cloud"] {
        assert!(catalog.get(id).is_none());
    }
    assert!(matches!(
        resolve(
            &catalog,
            "roo-code-vscode",
            Scope::ApplicationWorkspace,
            &context
        ),
        Err(ResolveError::UnsupportedScope { .. })
    ));
    context.user_home = None;
    assert!(matches!(
        resolve(&catalog, "roo-code-vscode", Scope::User, &context),
        Err(ResolveError::MissingContext(ContextBase::Home))
    ));
    context.project_root = None;
    assert!(matches!(
        resolve(&catalog, "roo-code-vscode", Scope::Project, &context),
        Err(ResolveError::MissingContext(ContextBase::Project))
    ));
}

#[test]
fn kilo_coincident_writes_preserve_both_variant_attributions() {
    coincident_writes_preserve_variants(["kilo-code-cli", "kilo-code-vscode"], false);
}
#[test]
fn kilo_new_platform_scopes_and_configuration_gaps() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = context();
    for variable in [
        "KILO_CONFIG_DIR",
        "KILO_HOME",
        "XDG_CONFIG_HOME",
        "CLINE_DIR",
        "CLAUDE_CONFIG_DIR",
    ] {
        context
            .overrides
            .insert(variable.into(), "unverified relocation".into());
    }
    for variant in ["cli", "vscode"] {
        let id = format!("kilo-code-{variant}");
        let target = catalog.get(&id).unwrap();
        assert_eq!(target.product_id, "kilo-code");
        assert_eq!(target.variant, variant);
        for (scope, root) in [
            (Scope::User, context.user_home.as_ref().unwrap()),
            (Scope::Project, context.project_root.as_ref().unwrap()),
        ] {
            let result = resolve(&catalog, &id, scope, &context).unwrap();
            assert_eq!(result.preferred_write.path, root.join(".kilo/skills"));
            assert_eq!(result.discovery.len(), 2);
            assert_eq!(result.discovery[0].location, result.preferred_write);
            assert_eq!(
                result.discovery[1].location.path,
                root.join(".agents/skills")
            );
            for reader in &result.discovery {
                assert_eq!(reader.traversal, DiscoveryTraversal::Exact);
                assert_eq!(
                    reader.location.evidence.source.as_ref().unwrap().url,
                    "https://kilo.ai/docs/customize/skills"
                );
                assert!(reader.location.evidence.discovery_tests.is_empty());
            }
            assert!(
                result
                    .warnings
                    .contains(&ResolutionWarning::HostDiscoveryUntested)
            );
            assert!(
                result
                    .warnings
                    .contains(&ResolutionWarning::PlacementNotExclusive)
            );
            assert!(
                !result
                    .warnings
                    .contains(&ResolutionWarning::ResolverEvidenceAbsent)
            );
        }
        assert!(
            target
                .limitations
                .iter()
                .any(|gap| gap.contains("KILO_CONFIG_DIR"))
        );
        assert!(
            target
                .limitations
                .iter()
                .any(|gap| gap.contains("Conditional Claude paths"))
        );
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
    for id in [
        "kilo",
        "kilocode",
        "kilo-code",
        "kilo-code-legacy",
        "kilo-code-jetbrains",
        "kilo-code-desktop",
        "kilo-code-cloud",
    ] {
        assert!(catalog.get(id).is_none());
    }
}

#[test]
fn kimi_code_native_and_alternate_writes_remain_distinct() {
    native_writes_with_override("kimi-code-cli", None);
    native_writes_with_override("kimi-code-cli", Some("KIMI_CODE_HOME"));
}
#[test]
fn kimi_code_current_roots_override_and_legacy_boundaries() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = context();
    for variable in [
        "KIMI_SHARE_DIR",
        "XDG_CONFIG_HOME",
        "CLINE_DIR",
        "CODEX_HOME",
    ] {
        context
            .overrides
            .insert(variable.into(), "legacy or unrelated relocation".into());
    }
    let target = catalog.get("kimi-code-cli").unwrap();
    assert_eq!(target.product_id, "kimi-code");
    assert_eq!(target.variant, "cli");
    for (scope, root) in [
        (Scope::User, context.user_home.as_ref().unwrap()),
        (Scope::Project, context.project_root.as_ref().unwrap()),
    ] {
        let resolved = resolve(&catalog, "kimi-code-cli", scope, &context).unwrap();
        assert_eq!(
            resolved.preferred_write.path,
            root.join(".kimi-code/skills")
        );
        assert_eq!(resolved.discovery.len(), 2);
        assert_eq!(resolved.discovery[0].location, resolved.preferred_write);
        assert_eq!(
            resolved.discovery[1].location.path,
            root.join(".agents/skills")
        );
        for reader in &resolved.discovery {
            assert_eq!(reader.traversal, DiscoveryTraversal::Exact);
            assert_eq!(
                reader.location.evidence.source.as_ref().unwrap().url,
                "https://moonshotai.github.io/kimi-code/en/customization/skills.html"
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
    let project = resolve(&catalog, "kimi-code-cli", Scope::Project, &context).unwrap();
    let alternate = context
        .user_home
        .as_ref()
        .unwrap()
        .join("alternate Kimi 日本語");
    context
        .overrides
        .insert("KIMI_CODE_HOME".into(), alternate.clone().into_os_string());
    let user = resolve(&catalog, "kimi-code-cli", Scope::User, &context).unwrap();
    assert_eq!(user.preferred_write.path, alternate.join("skills"));
    assert_eq!(
        user.preferred_write.origin,
        PathOrigin::Override("KIMI_CODE_HOME".into())
    );
    assert_eq!(user.discovery[0].location, user.preferred_write);
    assert_eq!(
        user.discovery[1].location.path,
        context.user_home.as_ref().unwrap().join(".agents/skills")
    );
    assert_eq!(
        project,
        resolve(&catalog, "kimi-code-cli", Scope::Project, &context).unwrap()
    );
    let mut no_home = context.clone();
    no_home.user_home = None;
    assert!(matches!(
        resolve(&catalog, "kimi-code-cli", Scope::User, &no_home),
        Err(ResolveError::MissingContext(ContextBase::Home))
    ));
    for value in ["", "relative", "~/.kimi-code"] {
        context
            .overrides
            .insert("KIMI_CODE_HOME".into(), value.into());
        assert!(matches!(
            resolve(&catalog, "kimi-code-cli", Scope::User, &context),
            Err(ResolveError::InvalidOverride { .. })
        ));
        assert_eq!(
            project,
            resolve(&catalog, "kimi-code-cli", Scope::Project, &context).unwrap()
        );
    }
    context.overrides.remove(&OsString::from("KIMI_CODE_HOME"));
    assert!(matches!(
        resolve(
            &catalog,
            "kimi-code-cli",
            Scope::ApplicationWorkspace,
            &context
        ),
        Err(ResolveError::UnsupportedScope { .. })
    ));
    context.user_home = None;
    assert!(matches!(
        resolve(&catalog, "kimi-code-cli", Scope::User, &context),
        Err(ResolveError::MissingContext(ContextBase::Home))
    ));
    context.project_root = None;
    assert!(matches!(
        resolve(&catalog, "kimi-code-cli", Scope::Project, &context),
        Err(ResolveError::MissingContext(ContextBase::Project))
    ));
    for id in [
        "kimi",
        "kimi-cli",
        "kimi-agent",
        "kimi-code",
        "kimi-code-ide",
        "kimi-code-web",
    ] {
        assert!(catalog.get(id).is_none());
    }
}

#[test]
fn junie_reader_alias_does_not_rewrite_native_installation() {
    native_writes_remain_distinct("junie-cli");
}
#[test]
fn junie_cli_default_roots_and_ide_configuration_gaps() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = context();
    for variable in [
        "JUNIE_HOME",
        "JUNIE_SKILL_LOCATIONS",
        "CLINE_DIR",
        "CODEX_HOME",
    ] {
        context
            .overrides
            .insert(variable.into(), "unverified or extra location".into());
    }
    context
        .overrides
        .insert("JUNIE_SKILL_DEFAULT_LOCATIONS".into(), "false".into());
    let target = catalog.get("junie-cli").unwrap();
    assert_eq!(target.product_id, "junie");
    assert_eq!(target.variant, "cli");
    assert!(
        target
            .limitations
            .iter()
            .any(|gap| gap.contains("JUNIE_HOME"))
    );
    assert!(
        target
            .limitations
            .iter()
            .any(|gap| gap.contains("Default roots can be disabled"))
    );
    for (scope, root) in [
        (Scope::User, context.user_home.as_ref().unwrap()),
        (Scope::Project, context.project_root.as_ref().unwrap()),
    ] {
        let result = resolve(&catalog, "junie-cli", scope, &context).unwrap();
        assert_eq!(result.preferred_write.path, root.join(".junie/skills"));
        assert_eq!(result.discovery.len(), 2);
        assert_eq!(result.discovery[0].location, result.preferred_write);
        assert_eq!(
            result.discovery[1].location.path,
            root.join(".agents/skills")
        );
        for reader in &result.discovery {
            assert_eq!(reader.traversal, DiscoveryTraversal::Exact);
            assert_eq!(
                reader.location.evidence.source.as_ref().unwrap().url,
                "https://junie.jetbrains.com/docs/agent-skills.html"
            );
            assert!(reader.location.evidence.discovery_tests.is_empty());
        }
        assert!(
            result
                .warnings
                .contains(&ResolutionWarning::HostDiscoveryUntested)
        );
        assert!(
            result
                .warnings
                .contains(&ResolutionWarning::PlacementNotExclusive)
        );
        assert!(
            !result
                .warnings
                .contains(&ResolutionWarning::ResolverEvidenceAbsent)
        );
    }
    for id in [
        "junie",
        "junie-ide",
        "junie-plugin",
        "junie-acp",
        "junie-cloud",
    ] {
        assert!(catalog.get(id).is_none());
    }
    assert!(matches!(
        resolve(&catalog, "junie-cli", Scope::ApplicationWorkspace, &context),
        Err(ResolveError::UnsupportedScope { .. })
    ));
    context.user_home = None;
    assert!(matches!(
        resolve(&catalog, "junie-cli", Scope::User, &context),
        Err(ResolveError::MissingContext(ContextBase::Home))
    ));
    context.project_root = None;
    assert!(matches!(
        resolve(&catalog, "junie-cli", Scope::Project, &context),
        Err(ResolveError::MissingContext(ContextBase::Project))
    ));
}

#[test]
fn factory_reader_alias_does_not_rewrite_native_installation() {
    native_writes_remain_distinct("factory-droid-cli");
}
#[test]
fn factory_droid_native_and_both_compatibility_roots() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = context();
    for variable in [
        "FACTORY_HOME",
        "FACTORY_CONFIG_DIR",
        "XDG_CONFIG_HOME",
        "CLINE_DIR",
        "CLAUDE_CONFIG_DIR",
    ] {
        context
            .overrides
            .insert(variable.into(), "unverified relocation".into());
    }
    let target = catalog.get("factory-droid-cli").unwrap();
    assert_eq!(target.product_id, "factory-droid");
    assert_eq!(target.variant, "cli");
    assert!(
        target
            .limitations
            .iter()
            .any(|gap| gap.contains("duplicate sanitized names"))
    );
    for (scope, root) in [
        (Scope::User, context.user_home.as_ref().unwrap()),
        (Scope::Project, context.project_root.as_ref().unwrap()),
    ] {
        let result = resolve(&catalog, "factory-droid-cli", scope, &context).unwrap();
        assert_eq!(result.preferred_write.path, root.join(".factory/skills"));
        assert_eq!(result.discovery.len(), 3);
        assert_eq!(result.discovery[0].location, result.preferred_write);
        for (reader, folder) in result
            .discovery
            .iter()
            .zip([".factory", ".agents", ".agent"])
        {
            assert_eq!(reader.location.path, root.join(folder).join("skills"));
            assert_eq!(reader.traversal, DiscoveryTraversal::Exact);
            assert_eq!(
                reader.location.evidence.source.as_ref().unwrap().url,
                "https://docs.factory.com/harness/skills"
            );
            assert!(reader.location.evidence.discovery_tests.is_empty());
        }
        assert!(
            result
                .warnings
                .contains(&ResolutionWarning::HostDiscoveryUntested)
        );
        assert!(
            result
                .warnings
                .contains(&ResolutionWarning::PlacementNotExclusive)
        );
        assert!(
            !result
                .warnings
                .contains(&ResolutionWarning::ResolverEvidenceAbsent)
        );
    }
    for id in [
        "factory",
        "droid",
        "factory-app",
        "factory-ide",
        "factory-cloud",
        "factory-mission",
    ] {
        assert!(catalog.get(id).is_none());
    }
    assert!(matches!(
        resolve(
            &catalog,
            "factory-droid-cli",
            Scope::ApplicationWorkspace,
            &context
        ),
        Err(ResolveError::UnsupportedScope { .. })
    ));
    context.user_home = None;
    assert!(matches!(
        resolve(&catalog, "factory-droid-cli", Scope::User, &context),
        Err(ResolveError::MissingContext(ContextBase::Home))
    ));
    context.project_root = None;
    assert!(matches!(
        resolve(&catalog, "factory-droid-cli", Scope::Project, &context),
        Err(ResolveError::MissingContext(ContextBase::Project))
    ));
}

#[test]
fn qwen_code_native_and_alternate_writes_are_not_fork_readers() {
    native_writes_with_expected_overlap("qwen-code-cli", None, |_| false);
    native_writes_with_expected_overlap("qwen-code-cli", Some("QWEN_HOME"), |_| false);
}
#[test]
fn qwen_code_home_override_and_unshared_reader_boundaries() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = context();
    for variable in [
        "QWEN_RUNTIME_DIR",
        "QWEN_CODE_HOME",
        "XDG_CONFIG_HOME",
        "GEMINI_CLI_HOME",
    ] {
        context
            .overrides
            .insert(variable.into(), "unrelated or unverified relocation".into());
    }
    let target = catalog.get("qwen-code-cli").unwrap();
    assert_eq!(target.product_id, "qwen-code");
    assert_eq!(target.variant, "cli");
    for (scope, root) in [
        (Scope::User, context.user_home.as_ref().unwrap()),
        (Scope::Project, context.project_root.as_ref().unwrap()),
    ] {
        let result = resolve(&catalog, "qwen-code-cli", scope, &context).unwrap();
        assert_eq!(result.preferred_write.path, root.join(".qwen/skills"));
        assert_eq!(result.discovery.len(), 1);
        assert_eq!(result.discovery[0].location, result.preferred_write);
        assert_eq!(result.discovery[0].traversal, DiscoveryTraversal::Exact);
        assert_eq!(
            result.preferred_write.evidence.source.as_ref().unwrap().url,
            if scope == Scope::User {
                "https://qwenlm.github.io/qwen-code-docs/en/users/configuration/settings/"
            } else {
                "https://qwenlm.github.io/qwen-code-docs/en/users/features/skills/"
            }
        );
        assert!(result.preferred_write.evidence.discovery_tests.is_empty());
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
    }
    let project = resolve(&catalog, "qwen-code-cli", Scope::Project, &context).unwrap();
    let alternate = context
        .user_home
        .as_ref()
        .unwrap()
        .join("Qwen alternate 日本語");
    context
        .overrides
        .insert("QWEN_HOME".into(), alternate.clone().into_os_string());
    context.user_home = None;
    let user = resolve(&catalog, "qwen-code-cli", Scope::User, &context).unwrap();
    assert_eq!(user.preferred_write.path, alternate.join("skills"));
    assert_eq!(
        user.preferred_write.origin,
        PathOrigin::Override("QWEN_HOME".into())
    );
    assert_eq!(user.discovery.len(), 1);
    assert_eq!(user.discovery[0].location, user.preferred_write);
    for value in ["", "relative", "~/.qwen"] {
        context.overrides.insert("QWEN_HOME".into(), value.into());
        assert!(matches!(
            resolve(&catalog, "qwen-code-cli", Scope::User, &context),
            Err(ResolveError::InvalidOverride { .. })
        ));
        assert_eq!(
            project,
            resolve(&catalog, "qwen-code-cli", Scope::Project, &context).unwrap()
        );
    }
    context.overrides.remove(&OsString::from("QWEN_HOME"));
    assert!(matches!(
        resolve(&catalog, "qwen-code-cli", Scope::User, &context),
        Err(ResolveError::MissingContext(ContextBase::Home))
    ));
    context.project_root = None;
    assert!(matches!(
        resolve(&catalog, "qwen-code-cli", Scope::Project, &context),
        Err(ResolveError::MissingContext(ContextBase::Project))
    ));
    assert!(matches!(
        resolve(
            &catalog,
            "qwen-code-cli",
            Scope::ApplicationWorkspace,
            &context
        ),
        Err(ResolveError::UnsupportedScope { .. })
    ));
    for id in [
        "qwen",
        "qwen-code",
        "qwen-code-ide",
        "qwen-code-cloud",
        "qwen-code-extension",
    ] {
        assert!(catalog.get(id).is_none());
    }
}

#[test]
fn pi_native_and_alternate_writes_remain_distinct() {
    native_writes_with_override("pi-cli", None);
    native_writes_with_override("pi-cli", Some("PI_CODING_AGENT_DIR"));
}
#[test]
fn pi_current_roots_override_and_reader_boundaries() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = context();
    for variable in [
        "PI_CODING_AGENT_SESSION_DIR",
        "XDG_CONFIG_HOME",
        "PI_PACKAGE_DIR",
        "PI_HOME",
    ] {
        context
            .overrides
            .insert(variable.into(), "legacy or unrelated relocation".into());
    }
    let target = catalog.get("pi-cli").unwrap();
    assert_eq!(target.product_id, "pi");
    assert_eq!(target.variant, "cli");
    for (scope, root) in [
        (Scope::User, context.user_home.as_ref().unwrap()),
        (Scope::Project, context.project_root.as_ref().unwrap()),
    ] {
        let resolved = resolve(&catalog, "pi-cli", scope, &context).unwrap();
        assert_eq!(
            resolved.preferred_write.path,
            root.join(if scope == Scope::User {
                ".pi/agent/skills"
            } else {
                ".pi/skills"
            })
        );
        assert_eq!(resolved.discovery.len(), 2);
        assert_eq!(
            resolved
                .warnings
                .contains(&ResolutionWarning::AncestorDiscoveryNotExpanded),
            scope == Scope::Project
        );
        assert_eq!(resolved.discovery[0].location, resolved.preferred_write);
        assert_eq!(
            resolved.discovery[1].location.path,
            root.join(".agents/skills")
        );
        for reader in &resolved.discovery {
            assert_eq!(
                reader.traversal,
                if scope == Scope::Project && reader.location.path == root.join(".agents/skills") {
                    DiscoveryTraversal::AncestorsToRepository
                } else {
                    DiscoveryTraversal::Exact
                }
            );
            assert_eq!(
                reader.location.evidence.source.as_ref().unwrap().url,
                if reader.location.path == root.join(".agents/skills") {
                    "https://pi.dev/docs/latest/skills"
                } else {
                    "https://pi.dev/docs/latest/configuration"
                }
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
    let project = resolve(&catalog, "pi-cli", Scope::Project, &context).unwrap();
    let alternate = context
        .user_home
        .as_ref()
        .unwrap()
        .join("alternate Pi 日本語");
    context.overrides.insert(
        "PI_CODING_AGENT_DIR".into(),
        alternate.clone().into_os_string(),
    );
    let user = resolve(&catalog, "pi-cli", Scope::User, &context).unwrap();
    assert_eq!(user.preferred_write.path, alternate.join("skills"));
    assert_eq!(
        user.preferred_write.origin,
        PathOrigin::Override("PI_CODING_AGENT_DIR".into())
    );
    assert_eq!(user.discovery[0].location, user.preferred_write);
    assert_eq!(
        user.discovery[1].location.path,
        context.user_home.as_ref().unwrap().join(".agents/skills")
    );
    assert_eq!(
        project,
        resolve(&catalog, "pi-cli", Scope::Project, &context).unwrap()
    );
    let mut no_home = context.clone();
    no_home.user_home = None;
    assert!(matches!(
        resolve(&catalog, "pi-cli", Scope::User, &no_home),
        Err(ResolveError::MissingContext(ContextBase::Home))
    ));
    for value in ["", "relative", "~/.pi/agent"] {
        context
            .overrides
            .insert("PI_CODING_AGENT_DIR".into(), value.into());
        assert!(matches!(
            resolve(&catalog, "pi-cli", Scope::User, &context),
            Err(ResolveError::InvalidOverride { .. })
        ));
        assert_eq!(
            project,
            resolve(&catalog, "pi-cli", Scope::Project, &context).unwrap()
        );
    }
    context
        .overrides
        .remove(&OsString::from("PI_CODING_AGENT_DIR"));
    assert!(matches!(
        resolve(&catalog, "pi-cli", Scope::ApplicationWorkspace, &context),
        Err(ResolveError::UnsupportedScope { .. })
    ));
    context.user_home = None;
    assert!(matches!(
        resolve(&catalog, "pi-cli", Scope::User, &context),
        Err(ResolveError::MissingContext(ContextBase::Home))
    ));
    context.project_root = None;
    assert!(matches!(
        resolve(&catalog, "pi-cli", Scope::Project, &context),
        Err(ResolveError::MissingContext(ContextBase::Project))
    ));
    for id in ["pi", "pi-sdk", "pi-ide", "pi-durable", "pi-android"] {
        assert!(catalog.get(id).is_none());
    }
}

#[test]
fn mistral_vibe_native_and_alternate_copy() {
    for variable in [None, Some("VIBE_HOME")] {
        native_writes_with_expected_overlap("mistral-vibe-cli", variable, |scope| {
            scope == Scope::Project
        });
    }
}

#[test]
fn mistral_vibe_scopes_override_and_reader_boundaries() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = context();
    for variable in ["MISTRAL_HOME", "XDG_CONFIG_HOME", "CODEX_HOME"] {
        context
            .overrides
            .insert(variable.into(), "unverified relocation".into());
    }
    let target = catalog.get("mistral-vibe-cli").unwrap();
    assert_eq!(target.product_id, "mistral-vibe");
    assert_eq!(target.variant, "cli");
    for (scope, root) in [
        (Scope::User, context.user_home.as_ref().unwrap()),
        (Scope::Project, context.project_root.as_ref().unwrap()),
    ] {
        let resolved = resolve(&catalog, "mistral-vibe-cli", scope, &context).unwrap();
        assert_eq!(resolved.preferred_write.path, root.join(".vibe/skills"));
        assert_eq!(
            resolved.discovery.len(),
            if scope == Scope::Project { 2 } else { 1 }
        );
        assert_eq!(resolved.discovery[0].location, resolved.preferred_write);
        if scope == Scope::Project {
            assert_eq!(
                resolved.discovery[1].location.path,
                root.join(".agents/skills")
            );
        }
        for reader in &resolved.discovery {
            assert_eq!(reader.traversal, DiscoveryTraversal::Exact);
            assert_eq!(
                reader.location.evidence.source.as_ref().unwrap().url,
                if scope == Scope::User {
                    "https://docs.mistral.ai/vibe/code/cli/configuration"
                } else {
                    "https://docs.mistral.ai/vibe/code/cli/skills"
                }
            );
            assert!(reader.location.evidence.discovery_tests.is_empty());
        }
        assert!(
            resolved
                .warnings
                .contains(&ResolutionWarning::HostDiscoveryUntested)
        );
        assert!(
            !resolved
                .warnings
                .contains(&ResolutionWarning::ResolverEvidenceAbsent)
        );
    }
    let project = resolve(&catalog, "mistral-vibe-cli", Scope::Project, &context).unwrap();
    let alternate = context
        .user_home
        .as_ref()
        .unwrap()
        .join("alternate Vibe 日本語");
    context
        .overrides
        .insert("VIBE_HOME".into(), alternate.clone().into_os_string());
    context.user_home = None;
    let user = resolve(&catalog, "mistral-vibe-cli", Scope::User, &context).unwrap();
    assert_eq!(user.preferred_write.path, alternate.join("skills"));
    assert_eq!(
        user.preferred_write.origin,
        PathOrigin::Override("VIBE_HOME".into())
    );
    assert_eq!(user.discovery.len(), 1);
    assert_eq!(user.discovery[0].location, user.preferred_write);
    for value in ["", "relative", "~/.vibe"] {
        context.overrides.insert("VIBE_HOME".into(), value.into());
        assert!(matches!(
            resolve(&catalog, "mistral-vibe-cli", Scope::User, &context),
            Err(ResolveError::InvalidOverride { .. })
        ));
        assert_eq!(
            project,
            resolve(&catalog, "mistral-vibe-cli", Scope::Project, &context).unwrap()
        );
    }
    context.overrides.remove(&OsString::from("VIBE_HOME"));
    assert!(matches!(
        resolve(&catalog, "mistral-vibe-cli", Scope::User, &context),
        Err(ResolveError::MissingContext(ContextBase::Home))
    ));
    context.project_root = None;
    assert!(matches!(
        resolve(&catalog, "mistral-vibe-cli", Scope::Project, &context),
        Err(ResolveError::MissingContext(ContextBase::Project))
    ));
    assert!(matches!(
        resolve(
            &catalog,
            "mistral-vibe-cli",
            Scope::ApplicationWorkspace,
            &context
        ),
        Err(ResolveError::UnsupportedScope { .. })
    ));
    for id in [
        "vibe",
        "mistral-vibe",
        "mistral-code",
        "vibe-work",
        "vibe-vscode",
        "vibe-web",
        "vibe-acp",
    ] {
        assert!(catalog.get(id).is_none());
    }
}

#[test]
fn amp_local_scopes_readers_and_platform_boundaries() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = context();
    let target = catalog.get("amp-cli").unwrap();
    assert_eq!(target.product_id, "amp");
    assert_eq!(target.variant, "cli");
    for scope in &target.scopes {
        assert_eq!(scope.platforms, vec![Platform::Linux, Platform::Macos]);
    }
    for id in [
        "amp",
        "amp-ide",
        "amp-app",
        "amp-orb",
        "amp-runner",
        "amp-ios",
        "amp-puck",
    ] {
        assert!(catalog.get(id).is_none());
    }
    if context.platform == Platform::Windows {
        for scope in [Scope::User, Scope::Project] {
            assert!(matches!(
                resolve(&catalog, "amp-cli", scope, &context),
                Err(ResolveError::UnsupportedPlatform(Platform::Windows))
            ));
        }
        return;
    }
    for variable in [
        "XDG_CONFIG_HOME",
        "AMP_HOME",
        "AMP_CONFIG_DIR",
        "CLAUDE_CONFIG_DIR",
    ] {
        context
            .overrides
            .insert(variable.into(), "unverified relocation".into());
    }
    for (scope, root, folders) in [
        (
            Scope::User,
            context.user_home.as_ref().unwrap(),
            vec![".config/agents", ".agents", ".config/amp", ".claude"],
        ),
        (
            Scope::Project,
            context.project_root.as_ref().unwrap(),
            vec![".agents", ".claude"],
        ),
    ] {
        let result = resolve(&catalog, "amp-cli", scope, &context).unwrap();
        assert_eq!(
            result.preferred_write.path,
            root.join(folders[0]).join("skills")
        );
        assert_eq!(result.discovery.len(), folders.len());
        for (reader, folder) in result.discovery.iter().zip(folders) {
            assert_eq!(reader.location.path, root.join(folder).join("skills"));
            assert_eq!(reader.traversal, DiscoveryTraversal::Exact);
            assert_eq!(
                reader.location.evidence.source.as_ref().unwrap().url,
                "https://ampcode.com/docs/customize/skills"
            );
            assert!(reader.location.evidence.discovery_tests.is_empty());
        }
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
    }
    assert!(matches!(
        resolve(&catalog, "amp-cli", Scope::ApplicationWorkspace, &context),
        Err(ResolveError::UnsupportedScope { .. })
    ));
    context.user_home = None;
    assert!(matches!(
        resolve(&catalog, "amp-cli", Scope::User, &context),
        Err(ResolveError::MissingContext(ContextBase::Home))
    ));
    context.project_root = None;
    assert!(matches!(
        resolve(&catalog, "amp-cli", Scope::Project, &context),
        Err(ResolveError::MissingContext(ContextBase::Project))
    ));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn amp_shared_project_and_distinct_user_copy() {
    for scope in [Scope::Project, Scope::User] {
        let fixture = Fixture::new();
        let source = fixture.0.join("source/sample");
        fs::create_dir_all(source.join("empty")).unwrap();
        fs::write(
            source.join("SKILL.md"),
            b"---\nname: sample\ndescription: Original inert local Amp fixture\n---\nData only.\n",
        )
        .unwrap();
        fs::write(
            source.join("resource café.txt"),
            b"original Amp fixture bytes",
        )
        .unwrap();
        let mut context = InstallContext::new(native_platform().unwrap());
        context.user_home = Some(fixture.0.join("home space"));
        context.project_root = Some(fixture.0.join("project 日本語"));
        fs::create_dir(context.user_home.as_ref().unwrap()).unwrap();
        fs::create_dir(context.project_root.as_ref().unwrap()).unwrap();
        let catalog = Catalog::bundled().unwrap();
        let sources = [inventory_skill(&source, InventoryLimits::default()).unwrap()];
        let preview = || {
            plan_install(
                &catalog,
                InstallRequest {
                    skills: &sources,
                    target_ids: &["amp-cli", "codex-cli", "amp-cli"],
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
        assert_eq!(
            plan.destinations().len(),
            if scope == Scope::Project { 1 } else { 2 }
        );
        let amp = resolve(&catalog, "amp-cli", scope, &context).unwrap();
        let codex = resolve(&catalog, "codex", scope, &context).unwrap();
        assert_eq!(
            amp.preferred_write.path == codex.preferred_write.path,
            scope == Scope::Project
        );
        assert!(
            amp.discovery
                .iter()
                .any(|reader| reader.location.path == codex.preferred_write.path)
        );
        if scope == Scope::Project {
            assert_eq!(
                plan.destinations()[0]
                    .attributions()
                    .iter()
                    .map(|a| {
                        assert_eq!(a.source_index, 0);
                        plan.targets()[a.target_index].id()
                    })
                    .collect::<Vec<_>>(),
                ["amp-cli", "codex"]
            );
        }
        assert!(
            plan.destinations()
                .iter()
                .all(|d| !d.path().unwrap().exists())
        );
        let report = apply_plan(&plan, ApplyOptions::default());
        assert!(report.is_successful(), "{report:?}");
        assert!(
            report
                .outcomes
                .iter()
                .all(|o| o.status == OutcomeStatus::Created)
        );
        for destination in plan.destinations() {
            let path = destination.path().unwrap();
            for name in ["SKILL.md", "resource café.txt"] {
                assert_eq!(
                    fs::read(path.join(name)).unwrap(),
                    fs::read(source.join(name)).unwrap()
                );
            }
            assert!(path.join("empty").is_dir());
        }
        let repeated = apply_plan(&preview(), ApplyOptions::default());
        assert!(repeated.is_successful(), "{repeated:?}");
        assert!(
            repeated
                .outcomes
                .iter()
                .all(|o| o.status == OutcomeStatus::Unchanged)
        );
        let resource = plan.destinations()[0]
            .path()
            .unwrap()
            .join("resource café.txt");
        fs::write(&resource, b"manual edit must remain").unwrap();
        let conflict = preview();
        assert!(!conflict.is_applicable());
        assert!(!apply_plan(&conflict, ApplyOptions::default()).is_successful());
        assert_eq!(fs::read(resource).unwrap(), b"manual edit must remain");
    }
}

#[cfg(target_os = "windows")]
#[test]
fn amp_native_windows_plan_is_rejected_without_writes() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source/sample");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("SKILL.md"),
        b"---\nname: sample\ndescription: Inert unsupported-host fixture\n---\nData only.\n",
    )
    .unwrap();
    let sources = [inventory_skill(&source, InventoryLimits::default()).unwrap()];
    let mut context = InstallContext::new(Platform::Windows);
    context.user_home = Some(fixture.0.join("home must remain absent"));
    context.project_root = Some(fixture.0.join("project must remain absent"));
    let catalog = Catalog::bundled().unwrap();
    for scope in [Scope::Project, Scope::User] {
        let plan = plan_install(
            &catalog,
            InstallRequest {
                skills: &sources,
                target_ids: &["amp-cli"],
                scope,
                context: &context,
                limits: PlanLimits::default(),
            },
        )
        .unwrap();
        assert!(!plan.is_applicable());
        assert!(!apply_plan(&plan, ApplyOptions::default()).is_successful());
        assert!(!context.user_home.as_ref().unwrap().exists());
        assert!(!context.project_root.as_ref().unwrap().exists());
    }
}
