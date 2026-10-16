//! Adapter-specific lexical tests; no host process, store or credential is used.
use std::path::PathBuf;

use agent_skills_kit::{catalog::*, resolver::*};

fn context() -> InstallContext {
    let mut context = InstallContext::new(native_platform().unwrap());
    // Derive absolute native syntax only; these paths are never created or read.
    let root = std::env::temp_dir().join("ask-core-targets-inert");
    context.project_root = Some(root.join("project 日本語"));
    context.user_home = Some(root.join("home space"));
    context
}

#[test]
fn codex_explicit_scopes_and_alias() {
    let catalog = Catalog::bundled().unwrap();
    let context = context();
    for (scope, root, traversal) in [
        (
            Scope::Project,
            context.project_root.as_ref().unwrap(),
            DiscoveryTraversal::AncestorsToRepository,
        ),
        (
            Scope::User,
            context.user_home.as_ref().unwrap(),
            DiscoveryTraversal::Exact,
        ),
    ] {
        let result = resolve(&catalog, "codex-cli", scope, &context).unwrap();
        assert_eq!(result, resolve(&catalog, "codex", scope, &context).unwrap());
        assert_eq!(result.target_id, "codex");
        assert_eq!(
            result.preferred_write.path,
            root.join(".agents").join("skills")
        );
        assert_eq!(result.discovery.len(), 1);
        assert_eq!(result.discovery[0].location, result.preferred_write);
        assert_eq!(result.discovery[0].traversal, traversal);
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
        assert_eq!(
            result
                .warnings
                .contains(&ResolutionWarning::AncestorDiscoveryNotExpanded),
            scope == Scope::Project
        );
        assert!(result.preferred_write.evidence.discovery_tests.is_empty());
        assert_eq!(
            result.preferred_write.evidence.source.unwrap().url,
            "https://learn.chatgpt.com/docs/build-skills"
        );
        assert!(result.warnings.iter().any(|warning| matches!(warning, ResolutionWarning::DocumentedLimitation(text) if text.contains("OpenCode"))));
    }
}

#[test]
fn codex_context_and_variant_boundaries() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = context();
    let expected = resolve(&catalog, "codex", Scope::User, &context).unwrap();
    // State storage documentation does not authorize a new authoring path rule.
    for variable in ["CODEX_HOME", "XDG_CONFIG_HOME", "HOME"] {
        context.overrides.insert(
            variable.into(),
            PathBuf::from("unreviewed-relative-root").into_os_string(),
        );
    }
    assert_eq!(
        resolve(&catalog, "codex", Scope::User, &context).unwrap(),
        expected
    );
    context.user_home = None;
    assert_eq!(
        resolve(&catalog, "codex", Scope::User, &context),
        Err(ResolveError::MissingContext(ContextBase::Home))
    );
    context.project_root = None;
    assert_eq!(
        resolve(&catalog, "codex", Scope::Project, &context),
        Err(ResolveError::MissingContext(ContextBase::Project))
    );
    assert!(matches!(
        resolve(&catalog, "codex", Scope::ApplicationWorkspace, &context),
        Err(ResolveError::UnsupportedScope { .. })
    ));
    for variant in ["codex-ide", "codex-cloud", "codex-admin", "codex-system"] {
        assert!(matches!(
            resolve(&catalog, variant, Scope::Project, &context),
            Err(ResolveError::UnknownTarget(_))
        ));
    }
}

#[test]
fn claude_scopes_and_config_override() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = context();
    let project = resolve(&catalog, "claude-code", Scope::Project, &context).unwrap();
    assert_eq!(
        project.preferred_write.path,
        context
            .project_root
            .as_ref()
            .unwrap()
            .join(".claude/skills")
    );
    assert_eq!(
        project.discovery[0].traversal,
        DiscoveryTraversal::AncestorsToRepository
    );
    let user = resolve(&catalog, "claude-code", Scope::User, &context).unwrap();
    assert_eq!(
        user.preferred_write.path,
        context.user_home.as_ref().unwrap().join(".claude/skills")
    );
    assert!(matches!(
        user.preferred_write.origin,
        PathOrigin::Fallback {
            base: ContextBase::Home,
            ..
        }
    ));
    assert_eq!(user.discovery[0].traversal, DiscoveryTraversal::Exact);
    let alternate = context
        .user_home
        .as_ref()
        .unwrap()
        .join("custom config 日本語");
    context.overrides.insert(
        "CLAUDE_CONFIG_DIR".into(),
        alternate.clone().into_os_string(),
    );
    context.user_home = None;
    let user = resolve(&catalog, "claude-code", Scope::User, &context).unwrap();
    assert_eq!(user.preferred_write.path, alternate.join("skills"));
    assert_eq!(
        user.preferred_write.origin,
        PathOrigin::Override("CLAUDE_CONFIG_DIR".into())
    );
    assert_eq!(user.discovery[0].location, user.preferred_write);
    assert_eq!(
        project,
        resolve(&catalog, "claude-code", Scope::Project, &context).unwrap()
    );
    assert!(
        user.warnings
            .contains(&ResolutionWarning::HostDiscoveryUntested)
    );
    assert!(
        !user
            .warnings
            .contains(&ResolutionWarning::ResolverEvidenceAbsent)
    );
    assert_eq!(
        user.preferred_write.evidence.source.unwrap().url,
        "https://code.claude.com/docs/en/claude-directory"
    );
}

#[test]
fn claude_missing_and_invalid_context() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = InstallContext::new(native_platform().unwrap());
    assert_eq!(
        resolve(&catalog, "claude-code", Scope::User, &context),
        Err(ResolveError::MissingContext(ContextBase::Home))
    );
    assert_eq!(
        resolve(&catalog, "claude-code", Scope::Project, &context),
        Err(ResolveError::MissingContext(ContextBase::Project))
    );
    for value in ["", "relative", "~/.claude"] {
        context
            .overrides
            .insert("CLAUDE_CONFIG_DIR".into(), value.into());
        assert!(matches!(
            resolve(&catalog, "claude-code", Scope::User, &context),
            Err(ResolveError::InvalidOverride { .. })
        ));
    }
    for variant in [
        "claude-code-cloud",
        "claude-code-desktop",
        "claude-code-enterprise",
    ] {
        assert!(matches!(
            resolve(&catalog, variant, Scope::User, &context),
            Err(ResolveError::UnknownTarget(_))
        ));
    }
}

#[test]
fn claude_reserved_names() {
    let catalog = Catalog::bundled().unwrap();
    let claude = catalog.get("claude-code").unwrap();
    for name in ["synced", "Synced", "SYNCED"] {
        assert_eq!(
            claude.reserved_skill_name(name),
            Some(ReservedSkillName::ClaudeSynced)
        );
    }
    for name in [
        "anthropic-skills",
        "anthropic-skills:pdf",
        "anthropic-skills:",
    ] {
        assert_eq!(
            claude.reserved_skill_name(name),
            Some(ReservedSkillName::ClaudeAccountNamespace)
        );
    }
    for name in ["synced-example", "anthropic-skills-extra", "pdf", "deploy"] {
        assert_eq!(claude.reserved_skill_name(name), None);
    }
    assert_eq!(
        catalog.get("codex").unwrap().reserved_skill_name("synced"),
        None
    );
    let mut custom = claude.clone();
    custom.variant = "plugin".into();
    assert_eq!(custom.reserved_skill_name("synced"), None);
    custom.variant = "cli".into();
    custom.product_id = "other".into();
    assert_eq!(custom.reserved_skill_name("synced"), None);
}

#[test]
fn opencode_native_writes_and_compatible_readers() {
    let catalog = Catalog::bundled().unwrap();
    let context = context();
    for (scope, root, native, traversal) in [
        (
            Scope::Project,
            context.project_root.as_ref().unwrap(),
            ".opencode/skills",
            DiscoveryTraversal::AncestorsToRepository,
        ),
        (
            Scope::User,
            context.user_home.as_ref().unwrap(),
            ".config/opencode/skills",
            DiscoveryTraversal::Exact,
        ),
    ] {
        let result = resolve(&catalog, "opencode", scope, &context).unwrap();
        assert_eq!(result.preferred_write.path, root.join(native));
        assert_eq!(result.discovery.len(), 3);
        assert_eq!(result.discovery[0].location, result.preferred_write);
        assert!(
            result
                .discovery
                .iter()
                .all(|read| read.traversal == traversal)
        );
        for (index, target, components) in [
            (1, "claude-code", ".claude/skills"),
            (2, "codex", ".agents/skills"),
        ] {
            let other = resolve(&catalog, target, scope, &context).unwrap();
            assert_eq!(result.discovery[index].location.path, root.join(components));
            assert_eq!(
                result.discovery[index].location.path,
                other.preferred_write.path
            );
            assert_ne!(result.preferred_write.path, other.preferred_write.path);
        }
        for read in &result.discovery {
            assert_eq!(
                read.location.evidence.source.as_ref().unwrap().url,
                "https://opencode.ai/docs/skills/"
            );
            assert!(!read.location.evidence.resolver_tests.is_empty());
            assert!(read.location.evidence.discovery_tests.is_empty());
        }
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
    }
}

#[test]
fn opencode_context_and_configuration_limits() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = context();
    let expected = resolve(&catalog, "opencode", Scope::User, &context).unwrap();
    for variable in [
        "XDG_CONFIG_HOME",
        "OPENCODE_CONFIG_DIR",
        "CLAUDE_CONFIG_DIR",
    ] {
        context.overrides.insert(
            variable.into(),
            context
                .user_home
                .as_ref()
                .unwrap()
                .join("alternate")
                .into_os_string(),
        );
    }
    let result = resolve(&catalog, "opencode", Scope::User, &context).unwrap();
    assert_eq!(result, expected);
    assert!(result.warnings.iter().any(|warning| matches!(warning, ResolutionWarning::DocumentedLimitation(text) if text.contains("OPENCODE_CONFIG_DIR") && text.contains("unverified"))));
    context.user_home = None;
    assert_eq!(
        resolve(&catalog, "opencode", Scope::User, &context),
        Err(ResolveError::MissingContext(ContextBase::Home))
    );
    context.project_root = None;
    assert_eq!(
        resolve(&catalog, "opencode", Scope::Project, &context),
        Err(ResolveError::MissingContext(ContextBase::Project))
    );
    for variant in ["opencode-ide", "opencode-cloud", "opencode-managed"] {
        assert!(matches!(
            resolve(&catalog, variant, Scope::User, &context),
            Err(ResolveError::UnknownTarget(_))
        ));
    }
}

#[test]
fn duplicate_selections_keep_native_writes_and_reader_attribution() {
    let catalog = Catalog::bundled().unwrap();
    let context = context();
    for scope in [Scope::Project, Scope::User] {
        let targets = resolve_targets(
            &catalog,
            ["opencode", "codex-cli", "claude-code", "codex", "opencode"],
            scope,
            &context,
        )
        .unwrap();
        assert_eq!(
            targets
                .iter()
                .map(|target| target.target_id.as_str())
                .collect::<Vec<_>>(),
            ["claude-code", "codex", "opencode"]
        );
        assert_eq!(
            targets,
            resolve_targets(
                &catalog,
                ["codex", "opencode", "claude-code"],
                scope,
                &context
            )
            .unwrap()
        );
        let writes = targets
            .iter()
            .map(|target| &target.preferred_write.path)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(writes.len(), 3);
        assert_eq!(
            targets[2].discovery[1].location.path,
            targets[0].preferred_write.path
        );
        assert_eq!(
            targets[2].discovery[2].location.path,
            targets[1].preferred_write.path
        );
        assert!(targets.iter().all(|target| {
            target
                .warnings
                .contains(&ResolutionWarning::PlacementNotExclusive)
        }));
    }
}

#[test]
fn selection_errors_and_empty_inputs_are_explicit() {
    let catalog = Catalog::bundled().unwrap();
    let context = context();
    assert_eq!(
        resolve_targets(&catalog, [], Scope::Project, &context).unwrap(),
        []
    );
    assert_eq!(
        resolve_targets(
            &catalog,
            ["codex", "unknown", "claude-code"],
            Scope::Project,
            &context
        ),
        Err(ResolveError::UnknownTarget("unknown".into()))
    );
    let empty = InstallContext::new(context.platform);
    assert_eq!(
        resolve_targets(&catalog, ["codex-cli", "codex"], Scope::Project, &empty),
        Err(ResolveError::MissingContext(ContextBase::Project))
    );
}

#[test]
fn distinct_target_attribution_is_retained_for_coincident_custom_writes() {
    let mut document = Catalog::bundled().unwrap().document();
    let codex_path = document
        .targets
        .iter()
        .find(|target| target.id == "codex")
        .unwrap()
        .scopes
        .iter()
        .find(|scope| scope.scope == Scope::Project)
        .unwrap()
        .preferred_write
        .as_ref()
        .unwrap()
        .clone();
    let custom = document
        .targets
        .iter_mut()
        .find(|target| target.id == "claude-code")
        .unwrap();
    // A synthetic customized rule, not a new host-path claim.
    custom.id = "synthetic-coincident".into();
    custom.product_id = "synthetic-coincident".into();
    custom
        .scopes
        .iter_mut()
        .find(|scope| scope.scope == Scope::Project)
        .unwrap()
        .preferred_write = Some(codex_path);
    let catalog = Catalog::new(document).unwrap();
    let result = resolve_targets(
        &catalog,
        ["synthetic-coincident", "codex-cli", "codex"],
        Scope::Project,
        &context(),
    )
    .unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].target_id, "codex");
    assert_eq!(result[1].target_id, "synthetic-coincident");
    assert_eq!(
        result[0].preferred_write.path,
        result[1].preferred_write.path
    );
}
