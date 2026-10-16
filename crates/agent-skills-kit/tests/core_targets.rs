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
