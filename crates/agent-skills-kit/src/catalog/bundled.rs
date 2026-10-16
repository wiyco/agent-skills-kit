use super::*;

struct Seed {
    id: &'static str,
    aliases: &'static [&'static str],
    url: &'static str,
    section: &'static str,
    project: &'static [&'static str],
    user: &'static [&'static str],
    ancestors: bool,
    compatibility: bool,
    status: SupportStatus,
    resolver_tests: &'static [&'static str],
}

impl Catalog {
    /// Load the bundled documentation candidates without filesystem/network access.
    ///
    /// Documented scopes have adapter resolver coverage, not proof of host
    /// discovery or safe installation. Other candidates remain unverified.
    ///
    /// ```
    /// use agent_skills_kit::catalog::{Catalog, SupportStatus};
    /// let catalog = Catalog::bundled()?;
    /// let codex = catalog.get("codex-cli").expect("bundled alias");
    /// assert!(codex.scopes.iter().all(|scope| scope.status == SupportStatus::Documented));
    /// # Ok::<(), agent_skills_kit::catalog::CatalogError>(())
    /// ```
    pub fn bundled() -> Result<Self, CatalogError> {
        let seeds = [
            Seed {
                id: "codex",
                aliases: &["codex-cli"],
                url: "https://learn.chatgpt.com/docs/build-skills",
                section: "Where Codex loads local skills",
                project: &[".agents", "skills"],
                user: &[".agents", "skills"],
                ancestors: true,
                compatibility: false,
                status: SupportStatus::Documented,
                resolver_tests: &[
                    "core_targets::codex_explicit_scopes_and_alias",
                    "core_targets::codex_context_and_variant_boundaries",
                ],
            },
            Seed {
                id: "claude-code",
                aliases: &[],
                url: "https://code.claude.com/docs/en/skills",
                section: "Choose where skills load",
                project: &[".claude", "skills"],
                user: &[".claude", "skills"],
                ancestors: true,
                compatibility: false,
                status: SupportStatus::Documented,
                resolver_tests: &[
                    "core_targets::claude_scopes_and_config_override",
                    "core_targets::claude_missing_and_invalid_context",
                    "core_targets::claude_reserved_names",
                ],
            },
            Seed {
                id: "opencode",
                aliases: &[],
                url: "https://opencode.ai/docs/skills/",
                section: "Place files; Understand discovery",
                project: &[".opencode", "skills"],
                user: &[".config", "opencode", "skills"],
                ancestors: true,
                compatibility: true,
                status: SupportStatus::Unverified,
                resolver_tests: &[],
            },
        ];
        Self::new(CatalogDocument {
            schema_version: SCHEMA_VERSION,
            targets: seeds.into_iter().map(Seed::target).collect(),
        })
    }
}

impl Seed {
    fn rule(&self, base: PathBase, components: &[&str]) -> EvidencedPathRule {
        EvidencedPathRule {
            path: PathRule {
                base,
                components: components.iter().map(|part| (*part).to_owned()).collect(),
            },
            evidence: RuleEvidence {
                documentation: DocumentationStatus::Reviewed,
                source: Some(EvidenceRecord {
                    url: self.url.to_owned(),
                    section: self.section.to_owned(),
                    accessed: "2026-10-16".to_owned(),
                    applicability: "Rolling CLI documentation; exact host version and OS applicability are not established".to_owned(),
                }),
                resolver_tests: self.resolver_tests.iter().map(|test| (*test).to_owned()).collect(),
                discovery_tests: Vec::new(),
                limitations: vec![if self.status == SupportStatus::Documented {
                    "Native lexical resolution tested; exact host versions, OS-specific discovery and real-host loading remain unverified".to_owned()
                } else {
                    "Documentation observations only; overrides and native adapter resolution still require review and testing".to_owned()
                }],
            },
        }
    }

    fn scope(&self, scope: Scope, base: PathBase, path: &[&str]) -> ScopeDefinition {
        let mut preferred = self.rule(base.clone(), path);
        if self.id == "claude-code" && scope == Scope::User {
            preferred.path = PathRule {
                base: PathBase::Environment {
                    variable: "CLAUDE_CONFIG_DIR".to_owned(),
                    fallback: Some(FallbackPath {
                        base: ContextBase::Home,
                        components: vec![".claude".to_owned()],
                    }),
                },
                components: vec!["skills".to_owned()],
            };
            let source = preferred.evidence.source.as_mut().expect("seed source");
            source.url = "https://code.claude.com/docs/en/claude-directory".to_owned();
            source.section = "Introduction; File reference".to_owned();
            source.applicability = "Rolling Claude Code CLI documentation; explicitly maps Windows home to USERPROFILE and all user .claude paths through CLAUDE_CONFIG_DIR; minimum host version not established".to_owned();
        }
        let traversal = if scope == Scope::Project && self.ancestors {
            DiscoveryTraversal::AncestorsToRepository
        } else {
            DiscoveryTraversal::Exact
        };
        let mut discovery = vec![DiscoveryLocation {
            location: preferred.clone(),
            traversal,
        }];
        if self.compatibility {
            for root in [".claude", ".agents"] {
                discovery.push(DiscoveryLocation {
                    location: self.rule(base.clone(), &[root, "skills"]),
                    traversal,
                });
            }
        }
        ScopeDefinition {
            scope,
            status: self.status,
            platforms: vec![Platform::Linux, Platform::Macos, Platform::Windows],
            preferred_write: Some(preferred),
            discovery,
            limitations: vec![
                "Platform lists describe native path resolution candidates; they do not establish host-version or OS discovery support".to_owned(),
            ],
        }
    }

    fn target(self) -> TargetDefinition {
        let scopes = vec![
            self.scope(Scope::Project, PathBase::Project, self.project),
            self.scope(Scope::User, PathBase::Home, self.user),
        ];
        let mut limitations = vec![
            "Only explicit local project/user placements are modeled; admin, bundled, plugin, cloud and nested/additional-directory semantics are not enabled".to_owned(),
            "Selecting a placement does not imply exclusive host visibility".to_owned(),
        ];
        if self.id == "codex" {
            limitations.extend([
                "OpenCode documents reading .agents/skills too; selecting Codex does not prevent OpenCode discovery".to_owned(),
                "Codex keeps duplicate skill names separate and follows skill-folder symlinks; this installer rejects descendant links".to_owned(),
                "CODEX_HOME is not interpreted as an authoring-path override: its state/skills description does not establish a replacement for $HOME/.agents/skills (environment-variables, Core locations; config-advanced, Config and state locations; learn.chatgpt.com/docs/config-file; reviewed 2026-10-16). Alternate or legacy state skill roots remain unverified".to_owned(),
            ]);
        }
        if self.id == "claude-code" {
            limitations.extend([
                "OpenCode documents reading .claude/skills; configured alternate Claude user roots are not implied OpenCode readers".to_owned(),
                "Claude local names synced (case-insensitive) and anthropic-skills or its colon namespace are reserved; check both folder and frontmatter names with reserved_skill_name before planning".to_owned(),
                "Enterprise overrides personal overrides project for duplicate local names; actual loading and configuration enablement are not tested".to_owned(),
                "Claude follows skill-folder symlinks; this installer rejects descendant links. Worktree fallback, nested/additional directories and plugin payload semantics are not modeled".to_owned(),
            ]);
        }
        TargetDefinition {
            id: self.id.to_owned(),
            aliases: self
                .aliases
                .iter()
                .map(|alias| (*alias).to_owned())
                .collect(),
            product_id: self.id.to_owned(),
            variant: "cli".to_owned(),
            scopes,
            limitations,
        }
    }
}
