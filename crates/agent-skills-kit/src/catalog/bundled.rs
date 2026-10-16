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
}

impl Catalog {
    /// Load the bundled documentation candidates without filesystem/network access.
    ///
    /// All scopes remain unverified for installation until resolver/platform
    /// coverage is implemented. Schema validation is not host discovery testing.
    ///
    /// ```
    /// use agent_skills_kit::catalog::{Catalog, SupportStatus};
    /// let catalog = Catalog::bundled()?;
    /// let codex = catalog.get("codex-cli").expect("bundled alias");
    /// assert!(codex.scopes.iter().all(|scope| scope.status == SupportStatus::Unverified));
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
            },
            Seed {
                id: "claude-code",
                aliases: &[],
                url: "https://code.claude.com/docs/en/skills",
                section: "Choose where skills load",
                project: &[".claude", "skills"],
                user: &[".claude", "skills"],
                ancestors: false,
                compatibility: false,
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
                resolver_tests: Vec::new(),
                discovery_tests: Vec::new(),
                limitations: vec![
                    "Documentation observations only; no resolver or real-host discovery tests yet".to_owned(),
                    "Overrides require separate review before enabling an adapter".to_owned(),
                ],
            },
        }
    }

    fn target(self) -> TargetDefinition {
        let scopes = [(Scope::Project, PathBase::Project, self.project), (Scope::User, PathBase::Home, self.user)]
            .into_iter().map(|(scope, base, path)| {
                let preferred = self.rule(base.clone(), path);
                let traversal = if scope == Scope::Project && self.ancestors {
                    DiscoveryTraversal::AncestorsToRepository
                } else {
                    DiscoveryTraversal::Exact
                };
                let mut discovery = vec![DiscoveryLocation { location: preferred.clone(), traversal }];
                if self.compatibility {
                    for root in [".claude", ".agents"] {
                        discovery.push(DiscoveryLocation { location: self.rule(base.clone(), &[root, "skills"]), traversal });
                    }
                }
                ScopeDefinition {
                    scope,
                    status: SupportStatus::Unverified,
                    platforms: vec![Platform::Linux, Platform::Macos, Platform::Windows],
                    preferred_write: Some(preferred),
                    discovery,
                    limitations: vec!["Platform candidates only; not advertised as supported before adapter review and native tests".to_owned()],
                }
            }).collect();
        TargetDefinition {
            id: self.id.to_owned(),
            aliases: self.aliases.iter().map(|alias| (*alias).to_owned()).collect(),
            product_id: self.id.to_owned(),
            variant: "cli".to_owned(),
            scopes,
            limitations: vec![
                "Only explicit local project/user placements are modeled; admin, bundled, plugin, cloud and nested/additional-directory semantics are not enabled".to_owned(),
                "Selecting a placement does not imply exclusive host visibility".to_owned(),
            ],
        }
    }
}
