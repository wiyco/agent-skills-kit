//! Bounded original generators, fixed seeds and invariant checks. No new dependency.
use agent_skills_kit::{
    catalog::*,
    receipt::*,
    resolver::*,
    validation::{MetadataLimits, parse_metadata},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    path::{Path, PathBuf},
};

const SEEDS: [u64; 4] = [1, 0x5eed, 0xc0de_cafe, 0x1234_5678_9abc_def0];
struct Generator(u64);
impl Generator {
    fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }
    fn index(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
    fn name(&mut self) -> String {
        let alphabet = ['a', 'b', 'z', '0', '9', 'β', '中'];
        let mut name = String::from("skill");
        for _ in 0..1 + self.index(4) {
            name.push('-');
            for _ in 0..1 + self.index(8) {
                name.push(alphabet[self.index(alphabet.len())]);
            }
        }
        name
    }
}
fn cases(count: usize, mut property: impl FnMut(u64, usize, &mut Generator)) {
    for seed in SEEDS {
        for case in 0..count {
            let mixed = seed
                .wrapping_add(case as u64)
                .wrapping_mul(0x9e37_79b9_7f4a_7c15);
            let mut generator = Generator(mixed.max(1));
            if let Err(panic) =
                catch_unwind(AssertUnwindSafe(|| property(seed, case, &mut generator)))
            {
                eprintln!(
                    "PROPERTY COUNTEREXAMPLE: seed={seed:#018x} case={case}; rerun this exact test with --nocapture"
                );
                resume_unwind(panic);
            }
        }
    }
}
fn native_path(text: impl AsRef<Path>) -> NativePath {
    NativePath::from_native(text.as_ref()).unwrap()
}
fn absolute(label: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\original property fixture").join(label)
    } else {
        PathBuf::from("/original property fixture").join(label)
    }
}

#[test]
fn seeded_skill_name_properties_reject_unsafe_mutations() {
    cases(64, |_, _, generator| {
        let name = generator.name();
        let document = |name: &str| {
            format!(
                "---\nname: {}\ndescription: Original seeded fixture\n---\nInert body.\n",
                serde_json::to_string(name).unwrap()
            )
            .into_bytes()
        };
        let parsed = parse_metadata(&document(&name), &name, MetadataLimits::default()).unwrap();
        assert_eq!(parsed.name(), name);
        for invalid in [
            format!("../{name}"),
            format!("{name}/child"),
            format!("{name}\\child"),
            format!("{name}:stream"),
            format!("-{name}"),
            format!("{name}-"),
            format!("{name}--extra"),
            format!("Upper-{name}"),
            format!("{name}\0"),
            format!("{name} "),
        ] {
            assert!(
                parse_metadata(&document(&invalid), &invalid, MetadataLimits::default()).is_err(),
                "unsafe name accepted: {invalid:?}"
            );
        }
    });
}

fn definition(base: PathBase, components: Vec<String>) -> TargetDefinition {
    let rule = EvidencedPathRule {
        path: PathRule { base, components },
        evidence: RuleEvidence {
            documentation: DocumentationStatus::Reviewed,
            source: Some(EvidenceRecord {
                url: "https://example.invalid/original-property-fixture".into(),
                section: "Original synthetic resolver data, not a host rule".into(),
                accessed: "2026-10-16".into(),
                applicability: "Pure native lexical tests only".into(),
            }),
            resolver_tests: vec![],
            discovery_tests: vec![],
            limitations: vec!["Synthetic test definition; no actual host support claim".into()],
        },
    };
    TargetDefinition {
        id: "property-fixture".into(),
        aliases: vec!["property-alias".into()],
        product_id: "property-fixture".into(),
        variant: "test".into(),
        scopes: vec![ScopeDefinition {
            scope: Scope::User,
            status: SupportStatus::Documented,
            platforms: vec![native_platform().unwrap()],
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
fn catalog(target: TargetDefinition) -> Catalog {
    Catalog::new(CatalogDocument {
        schema_version: SCHEMA_VERSION,
        targets: vec![target],
    })
    .unwrap()
}

#[test]
fn seeded_native_rule_properties_preserve_lexical_containment_and_explicit_overrides() {
    cases(64, |_, _, generator| {
        let components = (0..1 + generator.index(5))
            .map(|_| format!("segment-{:x}-中", generator.next() % 65536))
            .collect::<Vec<_>>();
        let base = if generator.index(2) == 0 {
            PathBase::Home
        } else {
            PathBase::Environment {
                variable: "ASK_PROPERTY_ROOT".into(),
                fallback: Some(FallbackPath {
                    base: ContextBase::Home,
                    components: vec![],
                }),
            }
        };
        let target = definition(base.clone(), components.clone());
        let catalog = catalog(target.clone());
        let home = absolute("home café");
        let custom = absolute("explicit override");
        let mut context = InstallContext::new(native_platform().unwrap());
        context.user_home = Some(home.clone());
        let overridden = matches!(base, PathBase::Environment { .. }) && generator.index(2) == 0;
        if overridden {
            context
                .overrides
                .insert("ASK_PROPERTY_ROOT".into(), custom.clone().into_os_string());
        }
        let root = if overridden { custom } else { home };
        let mut expected = root.clone();
        for component in &components {
            expected.push(component);
        }
        let resolved = resolve(&catalog, "property-alias", Scope::User, &context).unwrap();
        assert_eq!(resolved.preferred_write.path, expected);
        assert!(resolved.preferred_write.path.starts_with(&root));
        assert_eq!(
            resolve(&catalog, "property-fixture", Scope::User, &context).unwrap(),
            resolved
        );
        for unsafe_component in [
            "..",
            ".",
            "../escape",
            "absolute/name",
            "stream:name",
            "back\\slash",
            "",
        ] {
            let mut invalid = target.clone();
            invalid.scopes[0]
                .preferred_write
                .as_mut()
                .unwrap()
                .path
                .components
                .push(unsafe_component.into());
            assert!(
                Catalog::new(CatalogDocument {
                    schema_version: SCHEMA_VERSION,
                    targets: vec![invalid]
                })
                .is_err(),
                "unsafe rule accepted: {unsafe_component:?}"
            );
        }
        let explicit = catalog_env(components);
        context
            .overrides
            .insert("ASK_PROPERTY_ROOT".into(), "relative".into());
        assert!(
            matches!(
                resolve(&explicit, "property-fixture", Scope::User, &context),
                Err(ResolveError::InvalidOverride { .. })
            ),
            "present invalid override must not choose fallback"
        );
    });
}
fn catalog_env(components: Vec<String>) -> Catalog {
    catalog(definition(
        PathBase::Environment {
            variable: "ASK_PROPERTY_ROOT".into(),
            fallback: Some(FallbackPath {
                base: ContextBase::Home,
                components: vec![],
            }),
        },
        components,
    ))
}

#[test]
fn seeded_receipt_properties_round_trip_native_units_and_outcome_states() {
    cases(64, |seed, case, generator| {
        let name = generator.name();
        let destination = native_path(absolute(&format!("target/{name}")));
        let mut digest = [0u8; 32];
        for byte in &mut digest {
            *byte = generator.next() as u8;
        }
        // These are original synthetic digest records, not claims about files.
        let mut entries = vec![ReceiptEntry {
            path: native_path("SKILL.md"),
            kind: ReceiptEntryKind::File,
            mode: 0o644,
            bytes: generator.next() % 8192,
            digest: Some(ContentDigest::from_bytes(&digest)),
        }];
        for index in 0..generator.index(5) {
            entries.push(ReceiptEntry {
                path: native_path(format!("resource-{index}-café")),
                kind: ReceiptEntryKind::File,
                mode: [0o400, 0o600, 0o644, 0o755][generator.index(4)],
                bytes: generator.next() % 8192,
                digest: Some(ContentDigest::from_bytes(&digest)),
            });
        }
        if generator.index(2) == 0 {
            #[cfg(unix)]
            let opaque = NativePath::UnixBytes(vec![b'r', b'-', 0x80 + generator.index(128) as u8]);
            #[cfg(windows)]
            let opaque =
                NativePath::WindowsWide(vec![b'r' as u16, 0xd800 + generator.index(1024) as u16]);
            #[cfg(any(unix, windows))]
            entries.push(ReceiptEntry {
                path: opaque,
                kind: ReceiptEntryKind::File,
                mode: 0o600,
                bytes: 0,
                digest: Some(ContentDigest::from_bytes(&digest)),
            });
        }
        entries.push(ReceiptEntry {
            path: native_path("empty"),
            kind: ReceiptEntryKind::Directory,
            mode: 0o500,
            bytes: 0,
            digest: None,
        });
        let diagnostic = |code| ReceiptDiagnostic {
            code,
            path: Some(destination.clone()),
            operation: Some("original seeded fixture".into()),
            os_error: None,
        };
        let (planned, status, plan_diagnostics, failure, preflight) = match generator.index(6) {
            0 => (
                ReceiptPlanAction::Create,
                ReceiptStatus::Created,
                vec![],
                None,
                None,
            ),
            1 => (
                ReceiptPlanAction::Unchanged,
                ReceiptStatus::Unchanged,
                vec![],
                None,
                None,
            ),
            2 => (
                ReceiptPlanAction::Create,
                ReceiptStatus::Failed,
                vec![],
                Some(diagnostic(ReceiptFailureCode::Io)),
                None,
            ),
            3 => (
                ReceiptPlanAction::Create,
                ReceiptStatus::NotAttempted,
                vec![],
                None,
                Some(diagnostic(ReceiptFailureCode::Limit)),
            ),
            4 => (
                ReceiptPlanAction::Conflict,
                ReceiptStatus::NotAttempted,
                vec![diagnostic(ReceiptFailureCode::ExistingContentDiffers)],
                None,
                Some(diagnostic(ReceiptFailureCode::InapplicablePlan)),
            ),
            _ => (
                ReceiptPlanAction::Unsupported,
                ReceiptStatus::NotAttempted,
                vec![diagnostic(ReceiptFailureCode::TargetResolution)],
                None,
                Some(diagnostic(ReceiptFailureCode::InapplicablePlan)),
            ),
        };
        let document = ReceiptDocument {
            schema_version: RECEIPT_SCHEMA_VERSION,
            operation_id: OperationId::new(format!("property-{seed:x}-{case}")).unwrap(),
            platform: native_platform().unwrap(),
            scope: if generator.index(2) == 0 {
                Scope::User
            } else {
                Scope::Project
            },
            digest_algorithm: DigestAlgorithm::Blake3,
            sources: vec![ReceiptSource {
                root: native_path(absolute(&format!("source/{name}"))),
                name,
                root_mode: 0o755,
                entries,
            }],
            targets: vec!["original-property-fixture".into()],
            destinations: vec![ReceiptDestination {
                path: Some(destination),
                attributions: vec![ReceiptAttribution {
                    source: 0,
                    target: 0,
                }],
                planned_action: planned,
                plan_diagnostics,
                status,
                failure,
            }],
            preflight_failure: preflight,
            cleanup_diagnostics: vec![],
        };
        let receipt = Receipt::new(document.clone(), ReceiptLimits::default()).unwrap();
        let bytes = receipt.to_json(ReceiptLimits::default()).unwrap();
        let parsed = Receipt::from_json(&bytes, ReceiptLimits::default()).unwrap();
        assert_eq!(parsed.document(), &document);
        assert_eq!(parsed.to_json(ReceiptLimits::default()).unwrap(), bytes);
        let mut corrupted: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        corrupted["schema_version"] = 2.into();
        assert_eq!(
            Receipt::from_json(
                &serde_json::to_vec(&corrupted).unwrap(),
                ReceiptLimits::default()
            )
            .unwrap_err(),
            ReceiptError::UnsupportedVersion(2)
        );
    });
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod native {
    use super::*;
    use agent_skills_kit::{
        planner::*,
        validation::{InventoryLimits, SourceEntry, inventory_skill},
    };
    use std::{
        fs,
        os::unix::fs::MetadataExt,
        sync::atomic::{AtomicU64, Ordering},
    };
    struct Root(PathBuf);
    impl Root {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            for _ in 0..100 {
                let path = std::env::temp_dir().join(format!(
                    "ask-property-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
                    Err(error) => panic!("owned property root: {error}"),
                }
            }
            panic!("fixture collision budget")
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("owned property fixture teardown");
        }
    }
    fn tree(root: &Path) -> Vec<(PathBuf, Vec<u8>, u32)> {
        fn walk(root: &Path, path: &Path, output: &mut Vec<(PathBuf, Vec<u8>, u32)>) {
            let meta = fs::symlink_metadata(path).unwrap();
            output.push((
                path.strip_prefix(root).unwrap().to_owned(),
                if meta.is_file() {
                    fs::read(path).unwrap()
                } else {
                    Vec::new()
                },
                meta.mode(),
            ));
            if meta.is_dir() {
                for entry in fs::read_dir(path).unwrap() {
                    walk(root, &entry.unwrap().path(), output);
                }
            }
        }
        let mut output = Vec::new();
        walk(root, root, &mut output);
        output.sort_by(|a, b| a.0.cmp(&b.0));
        output
    }
    #[derive(Debug, Eq, PartialEq)]
    struct Row {
        path: Option<PathBuf>,
        action: PlannedAction,
        attribution: Vec<TargetAttribution>,
        writes: Vec<IntendedWrite>,
    }
    #[derive(Debug, Eq, PartialEq)]
    struct View {
        sources: Vec<(PathBuf, Vec<SourceEntry>)>,
        targets: Vec<(String, Vec<ResolutionWarning>)>,
        rows: Vec<Row>,
        writes: Vec<IntendedWrite>,
    }
    fn view(plan: &InstallPlan) -> View {
        View {
            sources: plan
                .sources()
                .iter()
                .map(|source| {
                    (
                        source.inventory().canonical_root().to_owned(),
                        source.inventory().entries().to_vec(),
                    )
                })
                .collect(),
            targets: plan
                .targets()
                .iter()
                .map(|target| (target.id().to_owned(), target.warnings().to_vec()))
                .collect(),
            rows: plan
                .destinations()
                .iter()
                .map(|destination| Row {
                    path: destination.path().map(Path::to_owned),
                    action: destination.action(),
                    attribution: destination.attributions().to_vec(),
                    writes: destination.intended_writes().to_vec(),
                })
                .collect(),
            writes: plan.intended_writes().to_vec(),
        }
    }

    #[test]
    fn seeded_planning_properties_are_read_only_and_permutation_invariant() {
        let catalog = Catalog::bundled().unwrap();
        cases(16, |seed, case, generator| {
            let root = Root::new();
            let mut skills = Vec::new();
            for index in 0..2 {
                let name = format!("skill-{index}");
                let source = root.0.join(format!("source-{index}")).join(&name);
                fs::create_dir_all(source.join("empty")).unwrap();
                fs::write(source.join("SKILL.md"),format!("---\nname: {name}\ndescription: Original property fixture\n---\nseed={seed:x} case={case}\n")).unwrap();
                fs::write(
                    source.join("reference café.txt"),
                    format!("original-{index}"),
                )
                .unwrap();
                skills.push(inventory_skill(&source, InventoryLimits::default()).unwrap());
            }
            let home = root.0.join("home with spaces");
            let project = root.0.join("project 中");
            fs::create_dir(&home).unwrap();
            fs::create_dir(&project).unwrap();
            let mut context = InstallContext::new(native_platform().unwrap());
            context.user_home = Some(home);
            context.project_root = Some(project);
            let scope = if generator.index(2) == 0 {
                Scope::User
            } else {
                Scope::Project
            };
            let mut targets = vec!["codex"];
            for target in ["claude-code", "opencode"] {
                if generator.index(2) == 0 {
                    targets.push(target);
                }
            }
            if generator.index(2) == 0 {
                targets.push("codex-cli");
            }
            let before = tree(&root.0);
            let first = plan_install(
                &catalog,
                InstallRequest {
                    skills: &skills,
                    target_ids: &targets,
                    scope,
                    context: &context,
                    limits: PlanLimits::default(),
                },
            )
            .unwrap();
            assert!(first.is_applicable());
            skills.reverse();
            targets.reverse();
            targets.push("codex");
            let second = plan_install(
                &catalog,
                InstallRequest {
                    skills: &skills,
                    target_ids: &targets,
                    scope,
                    context: &context,
                    limits: PlanLimits::default(),
                },
            )
            .unwrap();
            assert_eq!(view(&first), view(&second));
            assert_eq!(tree(&root.0), before);
        });
    }
}
