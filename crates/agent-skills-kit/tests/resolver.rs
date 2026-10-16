use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use agent_skills_kit::{catalog::*, resolver::*};

fn platform() -> Platform {
    native_platform().expect("native Linux/macOS/Windows test")
}

// Original synthetic registry: these rules are not evidence for a real host.
fn registry(scope: Scope, base: PathBase, components: &[&str]) -> Catalog {
    Catalog::new(CatalogDocument {
        schema_version: SCHEMA_VERSION,
        targets: vec![TargetDefinition {
            id: "synthetic".into(),
            aliases: vec!["synthetic-alias".into()],
            product_id: "synthetic".into(),
            variant: "fixture".into(),
            limitations: vec![],
            scopes: vec![ScopeDefinition {
                scope,
                status: SupportStatus::Documented,
                platforms: vec![Platform::Linux, Platform::Macos, Platform::Windows],
                preferred_write: Some(EvidencedPathRule {
                    path: PathRule {
                        base,
                        components: components.iter().map(|s| (*s).into()).collect(),
                    },
                    evidence: RuleEvidence {
                        documentation: DocumentationStatus::Reviewed,
                        source: Some(EvidenceRecord {
                            url: "https://example.invalid/synthetic".into(),
                            section: "Original fixture".into(),
                            accessed: "2026-10-16".into(),
                            applicability: "Synthetic resolver input only".into(),
                        }),
                        resolver_tests: vec![],
                        discovery_tests: vec![],
                        limitations: vec![],
                    },
                }),
                discovery: vec![],
                limitations: vec![],
            }],
        }],
    })
    .unwrap()
}

struct TempRoot(PathBuf);
impl TempRoot {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "ask-resolver-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create owned temporary root: {error}"),
            }
        }
        panic!("could not create a fresh temporary root")
    }
}
impl Drop for TempRoot {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove owned test root");
    }
}

#[test]
fn explicit_native_roots_spaces_unicode_and_nonexistent_destinations() {
    let root = TempRoot::new();
    let cases = [
        (Scope::Project, PathBase::Project, "worktree/module space"),
        (Scope::User, PathBase::Home, "home 日本語"),
        (
            Scope::ApplicationWorkspace,
            PathBase::ApplicationWorkspace,
            "application workspace",
        ),
    ];
    let before = fs::read_dir(&root.0).unwrap().count();
    for (scope, base, name) in cases {
        let catalog = registry(scope, base, &["synthetic-skills"]);
        let path = root.0.join(name);
        let mut context = InstallContext::new(platform());
        match scope {
            Scope::Project => context.project_root = Some(path.clone()),
            Scope::User => context.user_home = Some(path.clone()),
            Scope::ApplicationWorkspace => context.application_workspace = Some(path.clone()),
            _ => unreachable!(),
        }
        let result = resolve(&catalog, "synthetic-alias", scope, &context).unwrap();
        assert_eq!(result.target_id, "synthetic");
        assert_eq!(result.preferred_write.path, path.join("synthetic-skills"));
        assert!(!result.preferred_write.path.exists());
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
            result
                .warnings
                .contains(&ResolutionWarning::ResolverEvidenceAbsent)
        );
    }
    assert_eq!(before, fs::read_dir(&root.0).unwrap().count());
}

#[test]
fn missing_context_is_not_substituted_with_home_cwd_or_another_scope() {
    let context = InstallContext::new(platform());
    for (scope, base, missing) in [
        (Scope::Project, PathBase::Project, ContextBase::Project),
        (Scope::User, PathBase::Home, ContextBase::Home),
        (
            Scope::ApplicationWorkspace,
            PathBase::ApplicationWorkspace,
            ContextBase::ApplicationWorkspace,
        ),
    ] {
        let catalog = registry(scope, base, &["skills"]);
        assert_eq!(
            resolve(&catalog, "synthetic", scope, &context),
            Err(ResolveError::MissingContext(missing))
        );
    }
    let root = TempRoot::new();
    let mut context = context;
    context.project_root = Some(root.0.clone());
    let catalog = registry(
        Scope::ApplicationWorkspace,
        PathBase::ApplicationWorkspace,
        &["skills"],
    );
    assert_eq!(
        resolve(&catalog, "synthetic", Scope::ApplicationWorkspace, &context),
        Err(ResolveError::MissingContext(
            ContextBase::ApplicationWorkspace
        ))
    );
}

#[test]
fn only_explicit_absent_override_uses_documented_fallback() {
    let root = TempRoot::new();
    let catalog = registry(
        Scope::User,
        PathBase::Environment {
            variable: "EXAMPLE_CONFIG_ROOT".into(),
            fallback: Some(FallbackPath {
                base: ContextBase::Home,
                components: vec!["fallback-config".into()],
            }),
        },
        &["skills"],
    );
    let mut context = InstallContext::new(platform());
    context.user_home = Some(root.0.join("home"));
    let result = resolve(&catalog, "synthetic", Scope::User, &context).unwrap();
    assert_eq!(
        result.preferred_write.path,
        root.0.join("home/fallback-config/skills")
    );
    assert!(matches!(
        result.preferred_write.origin,
        PathOrigin::Fallback {
            base: ContextBase::Home,
            ..
        }
    ));
    let override_path = root.0.join("explicit config 日本語");
    context.overrides.insert(
        "EXAMPLE_CONFIG_ROOT".into(),
        override_path.clone().into_os_string(),
    );
    let result = resolve(&catalog, "synthetic", Scope::User, &context).unwrap();
    assert_eq!(result.preferred_write.path, override_path.join("skills"));
    assert_eq!(
        result.preferred_write.origin,
        PathOrigin::Override("EXAMPLE_CONFIG_ROOT".into())
    );
    for value in ["", "relative", "~", "$HOME", ".", ".."] {
        context
            .overrides
            .insert("EXAMPLE_CONFIG_ROOT".into(), value.into());
        assert!(
            matches!(
                resolve(&catalog, "synthetic", Scope::User, &context),
                Err(ResolveError::InvalidOverride { .. })
            ),
            "accepted override {value:?}"
        );
    }
}

#[test]
fn missing_override_without_fallback_and_unresolved_home_are_errors() {
    let mut context = InstallContext::new(platform());
    let catalog = registry(
        Scope::User,
        PathBase::Environment {
            variable: "EXAMPLE_ROOT".into(),
            fallback: None,
        },
        &["skills"],
    );
    assert_eq!(
        resolve(&catalog, "synthetic", Scope::User, &context),
        Err(ResolveError::MissingOverride("EXAMPLE_ROOT".into()))
    );
    let catalog = registry(
        Scope::User,
        PathBase::Environment {
            variable: "EXAMPLE_ROOT".into(),
            fallback: Some(FallbackPath {
                base: ContextBase::Home,
                components: vec![],
            }),
        },
        &["skills"],
    );
    assert_eq!(
        resolve(&catalog, "synthetic", Scope::User, &context),
        Err(ResolveError::MissingContext(ContextBase::Home))
    );
    context
        .overrides
        .insert("EXAMPLE_ROOT".into(), OsString::new());
    assert!(matches!(
        resolve(&catalog, "synthetic", Scope::User, &context),
        Err(ResolveError::InvalidOverride {
            issue: PathIssue::Empty,
            ..
        })
    ));
}

#[test]
fn unrelated_os_directory_overrides_cannot_change_a_literal_home_rule() {
    let root = TempRoot::new();
    let catalog = registry(
        Scope::User,
        PathBase::Home,
        &["documented-literal", "skills"],
    );
    let mut context = InstallContext::new(platform());
    context.user_home = Some(root.0.join("explicit home"));
    context.overrides = BTreeMap::from([
        (
            "XDG_CONFIG_HOME".into(),
            root.0.join("must-not-win").into_os_string(),
        ),
        (
            "HOME".into(),
            root.0.join("not-the-context-home").into_os_string(),
        ),
    ]);
    let result = resolve(&catalog, "synthetic", Scope::User, &context).unwrap();
    assert_eq!(
        result.preferred_write.path,
        root.0.join("explicit home/documented-literal/skills")
    );
}

#[test]
fn invalid_context_roots_are_rejected_without_normalizing_parent_components() {
    let root = TempRoot::new();
    let catalog = registry(Scope::Project, PathBase::Project, &["skills"]);
    for path in [
        PathBuf::new(),
        PathBuf::from("relative"),
        root.0.join("child/.."),
        root.0.join("nul\0name"),
    ] {
        let mut context = InstallContext::new(platform());
        context.project_root = Some(path);
        assert!(matches!(
            resolve(&catalog, "synthetic", Scope::Project, &context),
            Err(ResolveError::InvalidContext { .. })
        ));
    }
}

#[test]
fn unknown_targets_scopes_platforms_and_disabled_statuses_are_typed_errors() {
    let catalog = registry(Scope::Project, PathBase::Project, &["skills"]);
    let context = InstallContext::new(platform());
    assert!(matches!(
        resolve(&catalog, "missing", Scope::Project, &context),
        Err(ResolveError::UnknownTarget(_))
    ));
    assert!(matches!(
        resolve(&catalog, "synthetic", Scope::User, &context),
        Err(ResolveError::UnsupportedScope { .. })
    ));
    let foreign = if platform() == Platform::Linux {
        Platform::Macos
    } else {
        Platform::Linux
    };
    let mut other = context.clone();
    other.platform = foreign;
    assert!(matches!(
        resolve(&catalog, "synthetic", Scope::Project, &other),
        Err(ResolveError::ForeignPlatform { .. })
    ));
    let mut document = catalog.document();
    document.targets[0].scopes[0].platforms = vec![foreign];
    assert!(matches!(
        resolve(
            &Catalog::new(document).unwrap(),
            "synthetic",
            Scope::Project,
            &context
        ),
        Err(ResolveError::UnsupportedPlatform(_))
    ));
    for status in [
        SupportStatus::Unverified,
        SupportStatus::ImportOnly,
        SupportStatus::Unsupported,
        SupportStatus::ConfigurationRequired,
    ] {
        let mut document = catalog.document();
        let placement = &mut document.targets[0].scopes[0];
        placement.status = status;
        placement.limitations = vec!["Synthetic status condition".into()];
        if matches!(
            status,
            SupportStatus::ImportOnly | SupportStatus::Unsupported
        ) {
            placement.preferred_write = None;
        }
        let result = resolve(
            &Catalog::new(document).unwrap(),
            "synthetic",
            Scope::Project,
            &context,
        );
        assert!(matches!(
            result,
            Err(ResolveError::UnsupportedStatus(_)) | Err(ResolveError::ConfigurationRequired)
        ));
    }
    let mut document = catalog.document();
    document.targets[0].scopes[0].preferred_write = None;
    assert_eq!(
        resolve(
            &Catalog::new(document).unwrap(),
            "synthetic",
            Scope::Project,
            &context
        ),
        Err(ResolveError::NoWriteLocation)
    );
    assert!(
        resolve(
            &Catalog::bundled().unwrap(),
            "codex",
            Scope::Project,
            &context
        )
        .is_err()
    );
}

#[test]
fn discovery_and_provenance_do_not_replace_preferred_writes() {
    let root = TempRoot::new();
    let catalog = registry(Scope::Project, PathBase::Project, &["native", "skills"]);
    let mut document = catalog.document();
    let scope = &mut document.targets[0].scopes[0];
    let mut read = scope.preferred_write.clone().unwrap();
    read.path.components = vec!["compatible".into(), "skills".into()];
    read.evidence.limitations = vec!["Explicit synthetic limitation".into()];
    scope.discovery.push(DiscoveryLocation {
        location: read.clone(),
        traversal: DiscoveryTraversal::AncestorsToRepository,
    });
    scope.limitations = read.evidence.limitations.clone();
    let catalog = Catalog::new(document).unwrap();
    let mut context = InstallContext::new(platform());
    context.project_root = Some(root.0.join("worktree"));
    let first = resolve(&catalog, "synthetic", Scope::Project, &context).unwrap();
    assert_eq!(
        first.preferred_write.path,
        root.0.join("worktree/native/skills")
    );
    assert_eq!(
        first.discovery[0].location.path,
        root.0.join("worktree/compatible/skills")
    );
    assert_eq!(
        first.preferred_write.evidence,
        catalog.get("synthetic").unwrap().scopes[0]
            .preferred_write
            .as_ref()
            .unwrap()
            .evidence
    );
    assert!(
        first
            .warnings
            .contains(&ResolutionWarning::AncestorDiscoveryNotExpanded)
    );
    assert_eq!(
        first
            .warnings
            .iter()
            .filter(|warning| **warning
                == ResolutionWarning::DocumentedLimitation("Explicit synthetic limitation".into()))
            .count(),
        1
    );
    assert_eq!(
        first,
        resolve(&catalog, "synthetic", Scope::Project, &context).unwrap()
    );
}

#[test]
fn nested_cwd_is_ignored_in_a_child_process_without_global_mutation() {
    const CHILD_ROOT: &str = "ASK_RESOLVER_CHILD_ROOT";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let root = PathBuf::from(root);
        let catalog = registry(Scope::Project, PathBase::Project, &["skills"]);
        let mut context = InstallContext::new(platform());
        context.project_root = Some(root.join("worktree"));
        let result = resolve(&catalog, "synthetic", Scope::Project, &context).unwrap();
        assert_eq!(result.preferred_write.path, root.join("worktree/skills"));
        assert!(!result.preferred_write.path.exists());
        return;
    }
    let root = TempRoot::new();
    let nested = root.0.join("worktree/module/nested");
    fs::create_dir_all(&nested).unwrap();
    let before = std::env::current_dir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "nested_cwd_is_ignored_in_a_child_process_without_global_mutation",
        ])
        .current_dir(nested)
        .env(CHILD_ROOT, &root.0)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(std::env::current_dir().unwrap(), before);
    assert!(!root.0.join("worktree/skills").exists());
}

#[cfg(unix)]
#[test]
fn non_utf8_roots_and_overrides_preserve_native_identity() {
    use std::os::unix::ffi::OsStringExt;
    let root = TempRoot::new();
    let raw = root.0.join(OsString::from_vec(b"home-\xff".to_vec()));
    let mut context = InstallContext::new(platform());
    context.user_home = Some(raw.clone());
    let catalog = registry(Scope::User, PathBase::Home, &["skills"]);
    assert_eq!(
        resolve(&catalog, "synthetic", Scope::User, &context)
            .unwrap()
            .preferred_write
            .path,
        raw.join("skills")
    );
    let catalog = registry(
        Scope::User,
        PathBase::Environment {
            variable: "EXAMPLE_ROOT".into(),
            fallback: None,
        },
        &["skills"],
    );
    context
        .overrides
        .insert("EXAMPLE_ROOT".into(), raw.clone().into_os_string());
    assert_eq!(
        resolve(&catalog, "synthetic", Scope::User, &context)
            .unwrap()
            .preferred_write
            .path,
        raw.join("skills")
    );
}

#[cfg(windows)]
#[test]
fn native_windows_drive_unc_and_device_semantics_are_explicit() {
    let catalog = registry(Scope::Project, PathBase::Project, &["skills"]);
    for root in [
        r"C:\Skills Root",
        r"\\server\share\Skills",
        r"\\?\C:\Long Root",
        r"\\?\UNC\server\share\Skills",
    ] {
        let mut context = InstallContext::new(platform());
        context.project_root = Some(root.into());
        assert_eq!(
            resolve(&catalog, "synthetic", Scope::Project, &context)
                .unwrap()
                .preferred_write
                .path,
            PathBuf::from(root).join("skills")
        );
    }
    for root in [
        r"C:relative",
        r"\drive-relative",
        r"\\.\PhysicalDrive0",
        r"\\?\GLOBALROOT\Device",
        r"C:\CON",
        r"C:\NUL.txt",
        r"C:\NUL .txt",
        r"C:\COM¹",
        r"C:\LPT³.txt",
        r"C:\trailing.",
        r"C:\bad ",
        r"C:\bad:stream",
    ] {
        let mut context = InstallContext::new(platform());
        context.project_root = Some(root.into());
        assert!(
            matches!(
                resolve(&catalog, "synthetic", Scope::Project, &context),
                Err(ResolveError::InvalidContext { .. })
            ),
            "accepted native Windows root {root:?}"
        );
    }
    let mut context = InstallContext::new(platform());
    context.project_root = Some(r"C:\Root".into());
    let catalog = registry(Scope::Project, PathBase::Project, &["NUL.txt"]);
    assert!(matches!(
        resolve(&catalog, "synthetic", Scope::Project, &context),
        Err(ResolveError::InvalidRuleComponent(_))
    ));
}

#[cfg(windows)]
#[test]
fn native_windows_wide_roots_preserve_unpaired_surrogates() {
    use std::os::windows::ffi::OsStringExt;
    let root = PathBuf::from(r"C:\Root").join(OsString::from_wide(&[0xd800]));
    let catalog = registry(Scope::Project, PathBase::Project, &["skills"]);
    let mut context = InstallContext::new(platform());
    context.project_root = Some(root.clone());
    assert_eq!(
        resolve(&catalog, "synthetic", Scope::Project, &context)
            .unwrap()
            .preferred_write
            .path,
        root.join("skills")
    );
}
