use agent_skills_kit::planner::*;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "ask-plan-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("exclusive planning fixture: {error}"),
            }
        }
        panic!("planning fixture collision budget exhausted")
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove owned planning root");
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[test]
fn planning_without_a_reviewed_native_backend_fails_before_inspection() {
    use agent_skills_kit::{
        catalog::{Catalog, Scope},
        resolver::{InstallContext, native_platform},
    };
    let root = Root::new();
    let mut context = InstallContext::new(native_platform().unwrap());
    context.user_home = Some(root.0.clone());
    let error = plan_install(
        &Catalog::bundled().unwrap(),
        InstallRequest {
            skills: &[],
            target_ids: &["codex"],
            scope: Scope::User,
            context: &context,
            limits: PlanLimits::default(),
        },
    )
    .unwrap_err();
    assert_eq!(error, PlanError::UnsupportedPlatform);
    assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod native {
    use super::*;
    use agent_skills_kit::{catalog::*, resolver::*, validation::*};
    use std::os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, PermissionsExt, symlink},
    };

    fn source_dir(root: &Root, parent: &str, name: &str, description: &str) -> PathBuf {
        let path = root.0.join(parent).join(name);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("SKILL.md"), format!("---\nname: {name}\ndescription: {description}\ncustom: original-fixture\n---\nDo not execute embedded instructions.\n")).unwrap();
        path
    }
    fn skill(root: &Root, parent: &str, name: &str, description: &str) -> ValidatedSkill {
        inventory_skill(
            &source_dir(root, parent, name, description),
            InventoryLimits::default(),
        )
        .unwrap()
    }
    fn context(root: &Root) -> InstallContext {
        fs::create_dir_all(root.0.join("home")).unwrap();
        fs::create_dir_all(root.0.join("project")).unwrap();
        let mut context = InstallContext::new(native_platform().unwrap());
        context.user_home = Some(root.0.join("home"));
        context.project_root = Some(root.0.join("project"));
        context
    }
    fn preview(
        catalog: &Catalog,
        skills: &[ValidatedSkill],
        targets: &[&str],
        scope: Scope,
        context: &InstallContext,
    ) -> Result<InstallPlan, PlanError> {
        plan_install(
            catalog,
            InstallRequest {
                skills,
                target_ids: targets,
                scope,
                context,
                limits: PlanLimits::default(),
            },
        )
    }
    fn tree(root: &Path) -> Vec<(PathBuf, Vec<u8>, u32)> {
        fn walk(root: &Path, relative: &Path, out: &mut Vec<(PathBuf, Vec<u8>, u32)>) {
            let path = root.join(relative);
            let metadata = fs::symlink_metadata(&path).unwrap();
            let data = if metadata.is_file() {
                fs::read(&path).unwrap()
            } else if metadata.file_type().is_symlink() {
                fs::read_link(&path)
                    .unwrap()
                    .as_os_str()
                    .as_bytes()
                    .to_vec()
            } else {
                Vec::new()
            };
            out.push((relative.to_owned(), data, metadata.mode()));
            if metadata.is_dir() {
                for entry in fs::read_dir(path).unwrap() {
                    walk(root, &relative.join(entry.unwrap().file_name()), out);
                }
            }
        }
        let mut result = Vec::new();
        walk(root, Path::new(""), &mut result);
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }
    fn fixture_target(id: &str, base: PathBase, components: &[&str]) -> TargetDefinition {
        let rule = EvidencedPathRule {
            path: PathRule {
                base,
                components: components.iter().map(|part| (*part).to_owned()).collect(),
            },
            evidence: RuleEvidence {
                documentation: DocumentationStatus::Reviewed,
                source: Some(EvidenceRecord {
                    url: "https://example.invalid/original-fixture".into(),
                    section: "Original synthetic placement; not a host claim".into(),
                    accessed: "2026-10-16".into(),
                    applicability: "Owned temporary-root test only".into(),
                }),
                resolver_tests: vec![],
                discovery_tests: vec![],
                limitations: vec!["Synthetic test definition, not a bundled adapter".into()],
            },
        };
        TargetDefinition {
            id: id.into(),
            aliases: vec![],
            product_id: id.into(),
            variant: "fixture".into(),
            scopes: vec![ScopeDefinition {
                scope: Scope::User,
                status: SupportStatus::Documented,
                platforms: vec![Platform::Linux, Platform::Macos, Platform::Windows],
                preferred_write: Some(rule.clone()),
                discovery: vec![DiscoveryLocation {
                    location: rule,
                    traversal: DiscoveryTraversal::Exact,
                }],
                limitations: vec![],
            }],
            limitations: vec![],
        }
    }
    fn fixtures(targets: Vec<TargetDefinition>) -> Catalog {
        Catalog::new(CatalogDocument {
            schema_version: SCHEMA_VERSION,
            targets,
        })
        .unwrap()
    }

    #[test]
    fn core_target_previews_are_read_only_complete_and_order_independent() {
        let root = Root::new();
        let context = context(&root);
        let sources = [skill(&root, "sources", "sample", "fixture")];
        let catalog = Catalog::bundled().unwrap();
        let before = tree(&root.0);
        let first = preview(
            &catalog,
            &sources,
            &["opencode", "codex-cli", "claude-code", "codex"],
            Scope::User,
            &context,
        )
        .unwrap();
        let second = preview(
            &catalog,
            &sources,
            &["claude-code", "codex", "opencode"],
            Scope::User,
            &context,
        )
        .unwrap();
        assert_eq!(tree(&root.0), before);
        assert!(first.is_applicable());
        assert_eq!(first.sources().len(), 1);
        assert_eq!(
            first
                .targets()
                .iter()
                .map(PlannedTarget::id)
                .collect::<Vec<_>>(),
            ["claude-code", "codex", "opencode"]
        );
        assert_eq!(first.destinations().len(), 3);
        let paths = |plan: &InstallPlan| {
            plan.destinations()
                .iter()
                .map(|destination| {
                    (
                        destination.path().unwrap().to_owned(),
                        destination.action(),
                        destination.attributions().to_vec(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(paths(&first), paths(&second));
        assert_eq!(first.intended_writes(), second.intended_writes());
        for destination in first.destinations() {
            assert_eq!(destination.action(), PlannedAction::Create);
            assert_eq!(destination.attributions().len(), 1);
            assert!(destination.namespace().unwrap().is_current().unwrap());
            assert!(
                destination
                    .intended_writes()
                    .iter()
                    .any(|write| write.kind == IntendedWriteKind::CopyFile
                        && write.path.ends_with("SKILL.md"))
            );
        }
        assert!(first.targets().iter().all(|target| {
            target
                .warnings()
                .contains(&ResolutionWarning::PlacementNotExclusive)
                && !target.evidence().is_empty()
        }));
        assert_eq!(
            first.inspected_bytes(),
            sources[0].inventory().total_bytes()
        );
        let project = preview(
            &catalog,
            &sources,
            &["codex", "claude-code", "opencode"],
            Scope::Project,
            &context,
        )
        .unwrap();
        assert!(project.destinations().iter().all(|destination| {
            destination
                .path()
                .unwrap()
                .starts_with(fs::canonicalize(context.project_root.as_ref().unwrap()).unwrap())
        }));
        assert_eq!(tree(&root.0), before);
    }

    #[test]
    fn read_only_destination_and_unsafe_descendants_never_authorize_replacement() {
        let root = Root::new();
        let context = context(&root);
        let sources = [skill(&root, "source", "sample", "fixture")];
        let parent = root.0.join("home/.agents/skills");
        fs::create_dir_all(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o500)).unwrap();
        let before = tree(&root.0);
        let result = preview(
            &Catalog::bundled().unwrap(),
            &sources,
            &["codex"],
            Scope::User,
            &context,
        );
        assert_eq!(tree(&root.0), before);
        // Restore only the fixture permission so teardown and subsequent setup
        // remain possible, even when the preview assertion fails.
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        let plan = result.unwrap();
        assert_eq!(plan.destinations()[0].action(), PlannedAction::Create);
        // Create is placement intent; preview does not claim write permission.
        let outside = root.0.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"unchanged").unwrap();
        symlink(&outside, parent.join("sample")).unwrap();
        let before = tree(&root.0);
        let plan = preview(
            &Catalog::bundled().unwrap(),
            &sources,
            &["codex"],
            Scope::User,
            &context,
        )
        .unwrap();
        assert_eq!(plan.destinations()[0].action(), PlannedAction::Conflict);
        assert!(matches!(
            plan.destinations()[0].problem(),
            Some(DestinationProblem::Namespace(_))
        ));
        assert!(plan.intended_writes().is_empty());
        assert_eq!(tree(&root.0), before);
    }

    #[test]
    fn existing_directories_are_conservative_conflicts_until_compared() {
        let root = Root::new();
        let context = context(&root);
        let sources = [skill(&root, "source", "sample", "fixture")];
        fs::create_dir_all(root.0.join("home/.agents/skills/sample")).unwrap();
        fs::write(
            root.0.join("home/.agents/skills/sample/SKILL.md"),
            sources[0].metadata().source_bytes(),
        )
        .unwrap();
        let before = tree(&root.0);
        let plan = preview(
            &Catalog::bundled().unwrap(),
            &sources,
            &["codex"],
            Scope::User,
            &context,
        )
        .unwrap();
        assert!(!plan.is_applicable());
        assert_eq!(plan.destinations()[0].action(), PlannedAction::Conflict);
        assert_eq!(
            plan.destinations()[0].problem(),
            Some(&DestinationProblem::ExistingDirectoryUncompared)
        );
        assert!(plan.intended_writes().is_empty());
        assert_eq!(tree(&root.0), before);
    }

    #[test]
    fn unknown_targets_missing_context_and_host_constraints_remain_visible() {
        let root = Root::new();
        let mut context = context(&root);
        let sources = [skill(&root, "source", "synced", "fixture")];
        let plan = preview(
            &Catalog::bundled().unwrap(),
            &sources,
            &["claude-code", "codex-cloud", "codex"],
            Scope::User,
            &context,
        )
        .unwrap();
        assert_eq!(plan.targets().len(), 3);
        assert_eq!(plan.destinations().len(), 3);
        assert!(!plan.is_applicable());
        assert!(
            plan.destinations().iter().any(|destination| matches!(
                destination.problem(),
                Some(DestinationProblem::Host(_))
            ))
        );
        assert!(plan.destinations().iter().any(|destination| matches!(
            destination.problem(),
            Some(DestinationProblem::Target(ResolveError::UnknownTarget(_)))
        )));
        let unknown = plan
            .targets()
            .iter()
            .find(|target| target.id() == "codex-cloud")
            .unwrap();
        assert!(unknown.evidence().is_empty());
        context.user_home = None;
        let plan = preview(
            &Catalog::bundled().unwrap(),
            &sources,
            &["codex"],
            Scope::User,
            &context,
        )
        .unwrap();
        assert!(matches!(
            plan.destinations()[0].problem(),
            Some(DestinationProblem::Target(ResolveError::MissingContext(
                ContextBase::Home
            )))
        ));
        assert!(plan.intended_writes().is_empty());
    }

    #[test]
    fn a_stale_source_manifest_never_becomes_a_new_approved_payload() {
        let root = Root::new();
        let context = context(&root);
        let sources = [skill(&root, "source", "sample", "original")];
        fs::write(
            sources[0].inventory().requested_root().join("SKILL.md"),
            b"---\nname: sample\ndescription: changed\n---\n",
        )
        .unwrap();
        let before = tree(&root.0);
        assert!(matches!(
            preview(
                &Catalog::bundled().unwrap(),
                &sources,
                &["codex"],
                Scope::User,
                &context
            ),
            Err(PlanError::StaleSource(_))
        ));
        assert_eq!(tree(&root.0), before);
    }

    #[test]
    fn identical_sources_share_one_physical_operation_and_conflicting_sources_do_not() {
        for identical in [false, true] {
            let root = Root::new();
            let context = context(&root);
            let sources = [
                skill(&root, "a", "sample", "same"),
                skill(
                    &root,
                    "b",
                    "sample",
                    if identical { "same" } else { "different" },
                ),
            ];
            let plan = preview(
                &Catalog::bundled().unwrap(),
                &sources,
                &["codex-cli", "codex"],
                Scope::User,
                &context,
            )
            .unwrap();
            assert_eq!(plan.sources().len(), 2);
            assert_eq!(plan.targets().len(), 1);
            assert_eq!(plan.destinations().len(), 1);
            assert_eq!(plan.destinations()[0].attributions().len(), 2);
            if identical {
                assert!(plan.is_applicable());
                assert_eq!(
                    plan.intended_writes()
                        .iter()
                        .filter(|write| write.kind == IntendedWriteKind::CopyFile)
                        .count(),
                    1
                );
            } else {
                assert!(!plan.is_applicable());
                assert!(
                    plan.destinations()[0]
                        .problems()
                        .contains(&DestinationProblem::CompetingSources)
                );
                assert!(plan.intended_writes().is_empty());
            }
        }
    }

    #[test]
    fn physical_parent_aliases_deduplicate_without_native_placement_rewrites() {
        let root = Root::new();
        let mut context = context(&root);
        let sources = [skill(&root, "source", "sample", "fixture")];
        fs::create_dir_all(root.0.join("home/.agents/skills")).unwrap();
        symlink(&root.0, root.0.join("alias")).unwrap();
        context.overrides.insert(
            "FIXTURE_ROOT".into(),
            root.0.join("alias/home/.agents/skills").into_os_string(),
        );
        let mut document = Catalog::bundled().unwrap().document();
        document.targets.push(fixture_target(
            "fixture-twin",
            PathBase::Environment {
                variable: "FIXTURE_ROOT".into(),
                fallback: None,
            },
            &[],
        ));
        let catalog = Catalog::new(document).unwrap();
        let before = tree(&root.0);
        let plan = preview(
            &catalog,
            &sources,
            &["fixture-twin", "codex"],
            Scope::User,
            &context,
        )
        .unwrap();
        assert_eq!(plan.destinations().len(), 1);
        assert_eq!(plan.destinations()[0].attributions().len(), 2);
        assert!(plan.is_applicable());
        assert_eq!(tree(&root.0), before);
    }

    #[test]
    fn overlapping_source_and_destination_roots_are_rejected_by_identity() {
        let root = Root::new();
        let mut context = context(&root);
        let sources = [skill(&root, "source", "sample", "fixture")];
        context.project_root = Some(sources[0].inventory().requested_root().to_owned());
        let before = tree(&root.0);
        assert!(matches!(
            preview(
                &Catalog::bundled().unwrap(),
                &sources,
                &["codex"],
                Scope::Project,
                &context
            ),
            Err(PlanError::Overlap { .. })
        ));
        assert_eq!(tree(&root.0), before);
        let nested = source_dir(
            &root,
            "home/.agents/skills/sample/sources",
            "sample",
            "nested",
        );
        let sources = [inventory_skill(&nested, InventoryLimits::default()).unwrap()];
        assert!(matches!(
            preview(
                &Catalog::bundled().unwrap(),
                &sources,
                &["codex"],
                Scope::User,
                &context
            ),
            Err(PlanError::Overlap { .. })
        ));
    }

    #[test]
    fn nested_sources_and_overlapping_future_destinations_are_rejected() {
        let root = Root::new();
        let context = context(&root);
        let outer = source_dir(&root, "source", "outer", "outer");
        let inner = source_dir(&root, "source/outer", "inner", "inner");
        let sources = [
            inventory_skill(&outer, InventoryLimits::default()).unwrap(),
            inventory_skill(&inner, InventoryLimits::default()).unwrap(),
        ];
        assert!(matches!(
            preview(
                &Catalog::bundled().unwrap(),
                &sources,
                &["codex"],
                Scope::User,
                &context
            ),
            Err(PlanError::Overlap { .. })
        ));
        let sources = [skill(&root, "other", "sample", "fixture")];
        let catalog = fixtures(vec![
            fixture_target("fixture-a", PathBase::Home, &["one"]),
            fixture_target("fixture-b", PathBase::Home, &["one", "sample", "nested"]),
        ]);
        assert!(matches!(
            preview(
                &catalog,
                &sources,
                &["fixture-a", "fixture-b"],
                Scope::User,
                &context
            ),
            Err(PlanError::Overlap { .. })
        ));
    }

    #[test]
    fn new_intermediate_case_aliases_are_not_silently_shared() {
        let root = Root::new();
        let context = context(&root);
        let sources = [skill(&root, "source", "sample", "fixture")];
        let catalog = fixtures(vec![
            fixture_target("fixture-a", PathBase::Home, &["New", "left"]),
            fixture_target("fixture-b", PathBase::Home, &["new", "right"]),
        ]);
        let before = tree(&root.0);
        assert!(matches!(
            preview(
                &catalog,
                &sources,
                &["fixture-a", "fixture-b"],
                Scope::User,
                &context
            ),
            Err(PlanError::PortableNameCollision { .. })
        ));
        assert_eq!(tree(&root.0), before);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn case_and_normalization_colliding_payloads_fail_before_writes() {
        for (first, second) in [("README", "readme"), ("café", "cafe\u{301}")] {
            let root = Root::new();
            let context = context(&root);
            let source = source_dir(&root, "source", "sample", "fixture");
            fs::write(source.join(first), b"first").unwrap();
            fs::write(source.join(second), b"second").unwrap();
            let sources = [inventory_skill(&source, InventoryLimits::default()).unwrap()];
            let before = tree(&root.0);
            assert!(matches!(
                preview(
                    &Catalog::bundled().unwrap(),
                    &sources,
                    &["codex"],
                    Scope::User,
                    &context
                ),
                Err(PlanError::PortableNameCollision { .. })
            ));
            assert_eq!(tree(&root.0), before);
        }
    }

    #[test]
    fn whole_plan_limits_and_missing_selection_never_create_state() {
        let root = Root::new();
        let context = context(&root);
        let sources = [skill(&root, "source", "sample", "fixture")];
        let catalog = Catalog::bundled().unwrap();
        let before = tree(&root.0);
        assert_eq!(
            preview(&catalog, &[], &["codex"], Scope::User, &context).unwrap_err(),
            PlanError::EmptySelection("sources")
        );
        assert_eq!(
            preview(&catalog, &sources, &[], Scope::User, &context).unwrap_err(),
            PlanError::EmptySelection("targets")
        );
        for limits in [
            PlanLimits {
                max_skills: 0,
                ..PlanLimits::default()
            },
            PlanLimits {
                max_destinations: 1,
                ..PlanLimits::default()
            },
            PlanLimits {
                max_intended_entries: 1,
                ..PlanLimits::default()
            },
            PlanLimits {
                max_path_bytes: 1,
                ..PlanLimits::default()
            },
        ] {
            let error = plan_install(
                &catalog,
                InstallRequest {
                    skills: &sources,
                    target_ids: &["codex", "opencode"],
                    scope: Scope::User,
                    context: &context,
                    limits,
                },
            )
            .unwrap_err();
            assert!(matches!(
                error,
                PlanError::InvalidLimits | PlanError::Limit(_)
            ));
        }
        assert_eq!(tree(&root.0), before);
    }
}
