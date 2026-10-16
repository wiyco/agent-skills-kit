use super::TargetDefinition;

/// A documented core-host reservation, separate from general skill syntax.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ReservedSkillName {
    /// Claude Code reserves this folder name in every ASCII capitalization.
    ClaudeSynced,
    /// Claude Code reserves its account-skill namespace for non-plugin names.
    ClaudeAccountNamespace,
}

impl TargetDefinition {
    // OpenCode's regex and Zed Agent's a-z grammar are explicitly ASCII;
    // this narrows the general specification's Unicode name rule.
    pub(crate) fn requires_ascii_skill_name(&self) -> bool {
        (self.product_id == "opencode" && self.variant == "cli")
            || (self.product_id == "zed" && self.variant == "agent")
    }

    /// Identify a documented reservation for this local CLI product variant.
    ///
    /// Check both the folder and frontmatter name before planning a copy. `None`
    /// only means no reservation matched; it does not validate syntax, native
    /// filenames or discovery. Custom products and unmodeled variants have no
    /// built-in reservations. The bundled catalog's source/limitations describe
    /// applicability. Policies are bundled library behavior, not JSON rules.
    pub fn reserved_skill_name(&self, name: &str) -> Option<ReservedSkillName> {
        if self.product_id != "claude-code" || self.variant != "cli" {
            return None;
        }
        if name.eq_ignore_ascii_case("synced") {
            Some(ReservedSkillName::ClaudeSynced)
        } else if name == "anthropic-skills" || name.starts_with("anthropic-skills:") {
            Some(ReservedSkillName::ClaudeAccountNamespace)
        } else {
            None
        }
    }
}
