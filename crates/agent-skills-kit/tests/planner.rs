use agent_skills_kit::planner::*;
use std::{
    fs,
    path::PathBuf,
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

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
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
    use std::path::Path;

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
    fn identical_existing_payloads_are_unchanged_after_complete_comparison() {
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
        assert!(plan.is_applicable());
        assert_eq!(plan.destinations()[0].action(), PlannedAction::Unchanged);
        assert!(plan.destinations()[0].problems().is_empty());
        assert!(plan.destinations()[0].existing_payload().is_some());
        assert_eq!(
            plan.inspected_bytes(),
            sources[0].inventory().total_bytes() * 2
        );
        assert!(plan.intended_writes().is_empty());
        assert_eq!(tree(&root.0), before);
    }

    #[test]
    fn existing_comparison_covers_resources_empty_directories_modes_and_unsafe_entries() {
        for scenario in [
            "identical",
            "bytes",
            "extra",
            "missing",
            "directory",
            "mode",
            "root-mode",
            "privilege",
            "link",
            "hardlink",
            "invalid-metadata",
        ] {
            let root = Root::new();
            let context = context(&root);
            let source = source_dir(&root, "source", "sample", "fixture");
            fs::create_dir(source.join("empty")).unwrap();
            fs::write(source.join("resource"), b"original bytes").unwrap();
            fs::set_permissions(source.join("resource"), fs::Permissions::from_mode(0o640))
                .unwrap();
            let sources = [inventory_skill(&source, InventoryLimits::default()).unwrap()];
            let destination = root.0.join("home/.agents/skills/sample");
            fs::create_dir_all(destination.join("empty")).unwrap();
            for name in ["SKILL.md", "resource"] {
                fs::copy(source.join(name), destination.join(name)).unwrap();
            }
            match scenario {
                "bytes" => fs::write(destination.join("resource"), b"modified bytes").unwrap(),
                "extra" => fs::write(destination.join("extra"), b"extra").unwrap(),
                "missing" => fs::remove_file(destination.join("resource")).unwrap(),
                "directory" => fs::remove_dir(destination.join("empty")).unwrap(),
                "mode" => fs::set_permissions(
                    destination.join("resource"),
                    fs::Permissions::from_mode(0o600),
                )
                .unwrap(),
                "root-mode" => {
                    fs::set_permissions(&destination, fs::Permissions::from_mode(0o700)).unwrap()
                }
                "privilege" => fs::set_permissions(
                    destination.join("resource"),
                    fs::Permissions::from_mode(0o4640),
                )
                .unwrap(),
                "link" => symlink(&source, destination.join("outside")).unwrap(),
                "hardlink" => {
                    fs::hard_link(destination.join("resource"), destination.join("outside"))
                        .unwrap()
                }
                "invalid-metadata" => fs::write(destination.join("SKILL.md"), b"invalid").unwrap(),
                "identical" => (),
                _ => unreachable!(),
            }
            if scenario == "privilege" {
                assert_ne!(
                    fs::metadata(destination.join("resource")).unwrap().mode() & 0o4000,
                    0,
                    "fixture requires retained setuid bit; run outside a sandbox that strips it"
                );
            }
            let before = tree(&root.0);
            let plan = preview(
                &Catalog::bundled().unwrap(),
                &sources,
                &["codex"],
                Scope::User,
                &context,
            )
            .unwrap();
            assert_eq!(
                plan.destinations()[0].action(),
                if scenario == "identical" {
                    PlannedAction::Unchanged
                } else {
                    PlannedAction::Conflict
                },
                "{scenario}"
            );
            assert_eq!(plan.is_applicable(), scenario == "identical", "{scenario}");
            assert!(plan.intended_writes().is_empty(), "{scenario}");
            assert_eq!(tree(&root.0), before, "{scenario}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn existing_case_alias_is_not_claimed_as_an_identically_named_skill() {
        let root = Root::new();
        let context = context(&root);
        let sources = [skill(&root, "source", "sample", "fixture")];
        let destination = root.0.join("home/.agents/skills/SAMPLE");
        fs::create_dir_all(&destination).unwrap();
        fs::write(
            destination.join("SKILL.md"),
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
        assert_eq!(plan.destinations()[0].action(), PlannedAction::Conflict);
        assert!(matches!(
            plan.destinations()[0].problem(),
            Some(
                DestinationProblem::ExistingNameMismatch
                    | DestinationProblem::PortableNameCollision { .. }
            )
        ));
        assert_eq!(tree(&root.0), before);
    }

    #[test]
    fn existing_comparison_obeys_the_remaining_whole_preview_byte_budget() {
        let root = Root::new();
        let context = context(&root);
        let sources = [skill(&root, "source", "sample", "fixture")];
        let destination = root.0.join("home/.agents/skills/sample");
        fs::create_dir_all(&destination).unwrap();
        fs::copy(
            sources[0].inventory().requested_root().join("SKILL.md"),
            destination.join("SKILL.md"),
        )
        .unwrap();
        let total = sources[0].inventory().total_bytes();
        let before = tree(&root.0);
        for (allowed, expected) in [
            (total * 2, PlannedAction::Unchanged),
            (total * 2 - 1, PlannedAction::Conflict),
        ] {
            let plan = plan_install(
                &Catalog::bundled().unwrap(),
                InstallRequest {
                    skills: &sources,
                    target_ids: &["codex"],
                    scope: Scope::User,
                    context: &context,
                    limits: PlanLimits {
                        max_read_bytes: allowed,
                        ..PlanLimits::default()
                    },
                },
            )
            .unwrap();
            assert_eq!(plan.destinations()[0].action(), expected);
            assert!(plan.inspected_bytes() <= allowed);
            assert_eq!(tree(&root.0), before);
        }
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

#[cfg(windows)]
mod windows {
    use super::*;
    use agent_skills_kit::{catalog::*, resolver::*, validation::*};
    use std::{
        ffi::OsString,
        os::windows::{ffi::OsStringExt, fs::MetadataExt},
        path::Path,
    };

    fn source(root: &Root) -> PathBuf {
        let source = root.0.join("source 中").join("sample");
        fs::create_dir_all(source.join("empty folder")).unwrap();
        fs::write(source.join("SKILL.md"), b"---\nname: sample\ndescription: Original Windows planning fixture\n---\nInert original bytes.\n").unwrap();
        fs::write(source.join("reference café.txt"), b"original resource").unwrap();
        source
    }
    fn context(root: &Root) -> InstallContext {
        let home = root.0.join("home with spaces 中");
        let project = root.0.join("project");
        fs::create_dir(&home).unwrap();
        fs::create_dir(&project).unwrap();
        let mut context = InstallContext::new(native_platform().unwrap());
        context.user_home = Some(home);
        context.project_root = Some(project);
        context
    }
    fn preview(
        skills: &[ValidatedSkill],
        targets: &[&str],
        context: &InstallContext,
    ) -> Result<InstallPlan, PlanError> {
        plan_install(
            &Catalog::bundled().unwrap(),
            InstallRequest {
                skills,
                target_ids: targets,
                scope: Scope::User,
                context,
                limits: PlanLimits::default(),
            },
        )
    }
    fn tree(root: &Path) -> Vec<(PathBuf, Vec<u8>, u32)> {
        fn walk(root: &Path, path: &Path, result: &mut Vec<(PathBuf, Vec<u8>, u32)>) {
            let metadata = fs::symlink_metadata(path).unwrap();
            result.push((
                path.strip_prefix(root).unwrap().to_owned(),
                if metadata.is_file() {
                    fs::read(path).unwrap()
                } else {
                    vec![]
                },
                metadata.file_attributes(),
            ));
            if metadata.is_dir() {
                for entry in fs::read_dir(path).unwrap() {
                    walk(root, &entry.unwrap().path(), result);
                }
            }
        }
        let mut result = vec![];
        walk(root, root, &mut result);
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }
    #[test]
    fn core_preview_is_read_only_deduplicated_and_preserves_native_resource_names() {
        let root = Root::new();
        let source = source(&root);
        let opaque = OsString::from_wide(&[b'r' as u16, 0xd800]);
        fs::write(source.join(&opaque), b"opaque native resource").unwrap();
        let skills = [inventory_skill(&source, InventoryLimits::default()).unwrap()];
        let context = context(&root);
        let before = tree(&root.0);
        let plan = preview(
            &skills,
            &["opencode", "codex-cli", "claude-code", "codex"],
            &context,
        )
        .unwrap();
        assert!(plan.is_applicable(), "{plan:?}");
        assert_eq!(plan.targets().len(), 3);
        assert_eq!(plan.destinations().len(), 3);
        let home = fs::canonicalize(context.user_home.as_ref().unwrap()).unwrap();
        for relative in [
            ".agents/skills/sample",
            ".claude/skills/sample",
            ".opencode/skills/sample",
        ] {
            assert!(
                plan.destinations()
                    .iter()
                    .any(|row| row.path() == Some(home.join(relative).as_path())
                        && row.action() == PlannedAction::Create)
            );
        }
        assert_eq!(
            plan.intended_writes()
                .iter()
                .filter(|write| write.path.file_name() == Some(opaque.as_os_str()))
                .count(),
            3
        );
        let reordered = preview(&skills, &["codex", "claude-code", "opencode"], &context).unwrap();
        assert_eq!(plan.intended_writes(), reordered.intended_writes());
        assert_eq!(before, tree(&root.0));
    }
    #[test]
    fn existing_payload_is_unchanged_then_conflicts_without_repairing_manual_edits() {
        let root = Root::new();
        let source = source(&root);
        let skills = [inventory_skill(&source, InventoryLimits::default()).unwrap()];
        let context = context(&root);
        // Trusted fixture setup, not the library's still-gated copy executor.
        let target = context
            .user_home
            .as_ref()
            .unwrap()
            .join(".agents/skills/sample");
        fs::create_dir_all(target.join("empty folder")).unwrap();
        for name in ["SKILL.md", "reference café.txt"] {
            fs::write(target.join(name), fs::read(source.join(name)).unwrap()).unwrap();
        }
        let before = tree(&root.0);
        let plan = preview(&skills, &["codex"], &context).unwrap();
        assert_eq!(
            plan.destinations()[0].action(),
            PlannedAction::Unchanged,
            "{plan:?}"
        );
        assert!(plan.intended_writes().is_empty());
        assert_eq!(before, tree(&root.0));
        drop(plan);
        fs::write(target.join("reference café.txt"), b"manual edit").unwrap();
        let before = tree(&root.0);
        let plan = preview(&skills, &["codex"], &context).unwrap();
        assert_eq!(plan.destinations()[0].action(), PlannedAction::Conflict);
        assert!(
            plan.destinations()[0]
                .problems()
                .contains(&DestinationProblem::ExistingContentDiffers)
        );
        assert_eq!(before, tree(&root.0));
    }
    #[test]
    fn native_case_alias_never_becomes_unchanged_or_overwrite_intent() {
        let root = Root::new();
        let source = source(&root);
        let skills = [inventory_skill(&source, InventoryLimits::default()).unwrap()];
        let context = context(&root);
        fs::create_dir_all(
            context
                .user_home
                .as_ref()
                .unwrap()
                .join(".agents/skills/SAMPLE"),
        )
        .unwrap();
        let before = tree(&root.0);
        let plan = preview(&skills, &["codex"], &context).unwrap();
        assert_eq!(
            plan.destinations()[0].action(),
            PlannedAction::Conflict,
            "{plan:?}"
        );
        assert!(
            plan.destinations()[0]
                .problems()
                .iter()
                .any(|problem| matches!(
                    problem,
                    DestinationProblem::ExistingNameMismatch
                        | DestinationProblem::PortableNameCollision { .. }
                ))
        );
        assert!(plan.intended_writes().is_empty());
        assert_eq!(before, tree(&root.0));
    }
    #[test]
    fn overlap_normalization_collisions_and_budgets_fail_without_writes() {
        let root = Root::new();
        let source = source(&root);
        let mut context = context(&root);
        let skills = [inventory_skill(&source, InventoryLimits::default()).unwrap()];
        context.user_home = Some(source.clone());
        let before = tree(&root.0);
        assert!(matches!(
            preview(&skills, &["codex"], &context),
            Err(PlanError::Overlap { .. })
        ));
        assert_eq!(before, tree(&root.0));
        context.user_home = Some(root.0.join("home with spaces 中"));
        let error = plan_install(
            &Catalog::bundled().unwrap(),
            InstallRequest {
                skills: &skills,
                target_ids: &["codex"],
                scope: Scope::User,
                context: &context,
                limits: PlanLimits {
                    max_path_bytes: 1,
                    ..PlanLimits::default()
                },
            },
        )
        .unwrap_err();
        assert_eq!(error, PlanError::Limit("path bytes"));
        assert_eq!(before, tree(&root.0));
        drop(skills);
        fs::write(source.join("café"), b"first").unwrap();
        fs::write(source.join("cafe\u{301}"), b"second").unwrap();
        let skills = [inventory_skill(&source, InventoryLimits::default()).unwrap()];
        let before = tree(&root.0);
        assert!(matches!(
            preview(&skills, &["codex"], &context),
            Err(PlanError::PortableNameCollision { .. })
        ));
        assert_eq!(before, tree(&root.0));
    }
}
