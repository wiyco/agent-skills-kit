use agent_skills_kit::catalog::{Catalog, DocumentationStatus, PathBase, Scope, SupportStatus};

#[test]
fn bundled_candidates_are_deterministic_and_do_not_claim_tested_installation() {
    let first = Catalog::bundled().unwrap();
    let second = Catalog::bundled().unwrap();
    assert_eq!(first.document(), second.document());
    assert_eq!(
        first
            .targets()
            .map(|target| target.id.as_str())
            .collect::<Vec<_>>(),
        [
            "amp-cli",
            "claude-code",
            "cline-cli",
            "cline-vscode",
            "codex",
            "crush-cli",
            "cursor-ide",
            "factory-droid-cli",
            "gemini-cli",
            "github-copilot-cli",
            "github-copilot-vscode",
            "junie-cli",
            "kilo-code-cli",
            "kilo-code-vscode",
            "kimi-code-cli",
            "kiro-ide",
            "mistral-vibe-cli",
            "opencode",
            "pi-cli",
            "qwen-code-cli",
            "roo-code-vscode"
        ]
    );
    assert_eq!(first.get("codex-cli").unwrap().id, "codex");
    for target in first.targets() {
        for scope in &target.scopes {
            assert_eq!(scope.status, SupportStatus::Documented);
            for rule in scope
                .preferred_write
                .iter()
                .chain(scope.discovery.iter().map(|location| &location.location))
            {
                assert_eq!(rule.evidence.documentation, DocumentationStatus::Reviewed);
                assert!(rule.evidence.source.is_some());
                assert!(!rule.evidence.resolver_tests.is_empty());
                assert!(rule.evidence.discovery_tests.is_empty());
            }
        }
    }
}

#[test]
fn opencode_read_locations_do_not_replace_native_write_placement() {
    let catalog = Catalog::bundled().unwrap();
    let target = catalog.get("opencode").unwrap();
    let scope = target
        .scopes
        .iter()
        .find(|scope| scope.scope == Scope::Project)
        .unwrap();
    assert_eq!(
        scope.preferred_write.as_ref().unwrap().path.components,
        [".opencode", "skills"]
    );
    assert_eq!(
        scope
            .discovery
            .iter()
            .map(|read| read.location.path.components[0].as_str())
            .collect::<Vec<_>>(),
        [".opencode", ".claude", ".agents"]
    );
    let user = target
        .scopes
        .iter()
        .find(|scope| scope.scope == Scope::User)
        .unwrap();
    assert_eq!(
        user.preferred_write.as_ref().unwrap().path.components,
        [".config", "opencode", "skills"]
    );
    assert!(matches!(
        user.preferred_write.as_ref().unwrap().path.base,
        PathBase::Home
    ));
}

#[test]
fn synthetic_context_examples_are_separate_from_real_host_candidates() {
    let examples = Catalog::from_json(include_bytes!("fixtures/catalog-contexts.json")).unwrap();
    let xdg = examples.get("example-xdg").unwrap();
    assert!(matches!(
        xdg.scopes[0].preferred_write.as_ref().unwrap().path.base,
        PathBase::Environment { .. }
    ));
    let workspace = examples.get("example-workspace").unwrap();
    assert_eq!(workspace.scopes[0].scope, Scope::ApplicationWorkspace);
    assert!(matches!(
        workspace.scopes[0]
            .preferred_write
            .as_ref()
            .unwrap()
            .path
            .base,
        PathBase::ApplicationWorkspace
    ));
    for target in examples.targets() {
        assert!(Catalog::bundled().unwrap().get(&target.id).is_none());
        for scope in &target.scopes {
            assert_eq!(scope.status, SupportStatus::Unverified);
            assert!(
                scope
                    .preferred_write
                    .as_ref()
                    .unwrap()
                    .evidence
                    .source
                    .is_none()
            );
        }
    }
}
