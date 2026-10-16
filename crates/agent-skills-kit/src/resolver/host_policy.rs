//! Host-specific refusal rules; path selection remains in catalog metadata.
use std::path::{Component, Path};

use caseless::Caseless;
use unicode_normalization::UnicodeNormalization;

use super::{InstallContext, PathIssue, PathOrigin, ResolveError, context_root};
use crate::catalog::{ContextBase, Scope, TargetDefinition};

pub(super) fn validate(
    target: &TargetDefinition,
    scope: Scope,
    context: &InstallContext,
    origin: &PathOrigin,
) -> Result<(), ResolveError> {
    if target.product_id != "kiro" || target.variant != "cli" || scope != Scope::User {
        return Ok(());
    }
    let PathOrigin::Override(variable) = origin else {
        return Ok(());
    };
    if variable != "KIRO_HOME" {
        return Ok(());
    }
    let value = context
        .overrides
        .get(variable)
        .ok_or_else(|| ResolveError::MissingOverride(variable.clone()))?;
    let root = Path::new(value);
    let home = context_root(ContextBase::Home, context)?;
    let rejected = || ResolveError::InvalidOverride {
        variable: variable.clone(),
        issue: PathIssue::HostOverridePolicy,
    };
    // Verbatim prefixes have equivalent ordinary spellings but different lexical
    // components. Do not let that ambiguity bypass the home/ancestor comparison.
    #[cfg(windows)]
    if [root, home.as_path()].into_iter().any(|path| {
        matches!(path.components().next(), Some(Component::Prefix(prefix)) if prefix.kind().is_verbatim())
    }) {
        return Err(rejected());
    }
    // Kiro V3 ignores roots, home itself and its ancestors. Refuse instead of
    // silently falling back. Unicode/case folding is deliberately conservative,
    // not a claim to reproduce native collation or canonicalize filesystem links.
    let root_parts = folded_components(root).ok_or_else(rejected)?;
    let home_parts = folded_components(&home).ok_or_else(rejected)?;
    if !root
        .components()
        .any(|part| matches!(part, Component::Normal(_)))
        || home_parts.starts_with(&root_parts)
    {
        return Err(rejected());
    }
    Ok(())
}

fn folded_components(path: &Path) -> Option<Vec<String>> {
    path.components()
        .map(|part| {
            Some(
                part.as_os_str()
                    .to_str()?
                    .nfd()
                    .default_case_fold()
                    .nfd()
                    .collect(),
            )
        })
        .collect()
}
