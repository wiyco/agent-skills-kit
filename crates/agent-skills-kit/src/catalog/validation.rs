use std::{collections::BTreeMap, error::Error, fmt};

use super::*;

/// Catalog parse or semantic validation failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum CatalogError {
    /// JSON syntax/type/unknown-field error with a location.
    Parse(serde_json::Error),
    /// Bounded parser input was exceeded.
    InputTooLarge,
    /// A version this implementation does not understand.
    UnsupportedVersion(u32),
    /// An ID or alias was already used in the global namespace.
    DuplicateName(String),
    /// Two definitions claim the same product/variant identity.
    DuplicateVariant(String, String),
    /// A definition violates an invariant.
    InvalidDefinition {
        /// Affected target ID.
        target: String,
        /// Affected field or invariant.
        field: &'static str,
    },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "invalid catalog JSON: {error}"),
            Self::InputTooLarge => f.write_str("catalog exceeds the 2 MiB parser limit"),
            Self::UnsupportedVersion(version) => write!(f, "unsupported catalog version {version}"),
            Self::DuplicateName(name) => write!(f, "duplicate target ID or alias: {name}"),
            Self::DuplicateVariant(product, variant) => {
                write!(f, "duplicate product variant: {product}/{variant}")
            }
            Self::InvalidDefinition { target, field } => {
                write!(f, "invalid {field} in target {target}")
            }
        }
    }
}

impl Error for CatalogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            _ => None,
        }
    }
}

/// Validated, immutable catalog with deterministic ID ordering and alias lookup.
#[derive(Clone, Debug)]
pub struct Catalog {
    targets: BTreeMap<String, TargetDefinition>,
    aliases: BTreeMap<String, String>,
}

impl Catalog {
    /// Parse at most 2 MiB of JSON, rejecting unknown fields and invalid rules.
    /// No network, filesystem, or environment access occurs.
    pub fn from_json(bytes: &[u8]) -> Result<Self, CatalogError> {
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(CatalogError::InputTooLarge);
        }
        Self::new(serde_json::from_slice(bytes).map_err(CatalogError::Parse)?)
    }

    /// Validate a document without changing any external state.
    pub fn new(document: CatalogDocument) -> Result<Self, CatalogError> {
        if document.schema_version != SCHEMA_VERSION {
            return Err(CatalogError::UnsupportedVersion(document.schema_version));
        }
        let mut targets = BTreeMap::new();
        let mut names = BTreeMap::new();
        let mut variants = std::collections::BTreeSet::new();
        for mut target in document.targets {
            validate_target(&target)?;
            for name in std::iter::once(&target.id).chain(&target.aliases) {
                if names.insert(name.clone(), target.id.clone()).is_some() {
                    return Err(CatalogError::DuplicateName(name.clone()));
                }
            }
            if !variants.insert((target.product_id.clone(), target.variant.clone())) {
                return Err(CatalogError::DuplicateVariant(
                    target.product_id,
                    target.variant,
                ));
            }
            target.aliases.sort();
            target.scopes.sort_by_key(|scope| scope.scope);
            for scope in &mut target.scopes {
                scope.platforms.sort();
            }
            targets.insert(target.id.clone(), target);
        }
        Ok(Self {
            targets,
            aliases: names,
        })
    }

    /// Find by stable ID or explicit alias. Unknown IDs remain unknown.
    pub fn get(&self, name: &str) -> Option<&TargetDefinition> {
        self.aliases.get(name).and_then(|id| self.targets.get(id))
    }

    /// Iterate in stable lexical ID order, independent of input order.
    pub fn targets(&self) -> impl ExactSizeIterator<Item = &TargetDefinition> {
        self.targets.values()
    }

    /// Return the schema document in deterministic target order.
    pub fn document(&self) -> CatalogDocument {
        CatalogDocument {
            schema_version: SCHEMA_VERSION,
            targets: self.targets.values().cloned().collect(),
        }
    }
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
}

fn components(values: &[String]) -> bool {
    values.len() <= 64
        && values.iter().all(|value| {
            !value.is_empty()
                && value != "."
                && value != ".."
                && !value.contains(['/', '\\', ':'])
                && !value.chars().any(char::is_control)
        })
}

fn date(value: &str) -> bool {
    if value.len() != 10 || !value.is_ascii() {
        return false;
    }
    let parts: Vec<_> = value.split('-').collect();
    if parts.len() != 3
        || parts[0].len() != 4
        || parts[1].len() != 2
        || parts[2].len() != 2
        || parts
            .iter()
            .any(|part| !part.bytes().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    let numbers: Option<Vec<u32>> = parts.iter().map(|part| part.parse().ok()).collect();
    let Some(numbers) = numbers else { return false };
    let (year, month, day) = (numbers[0], numbers[1], numbers[2]);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => 0,
    };
    year != 0 && day != 0 && day <= days
}

fn documentation_url(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("https://") else {
        return false;
    };
    if value
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
    {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = if let Some((host, port)) = authority.split_once(':') {
        if port.is_empty()
            || !port.bytes().all(|c| c.is_ascii_digit())
            || port.parse::<u16>().is_err()
        {
            return false;
        }
        host
    } else {
        authority
    };
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
}

fn validate_target(target: &TargetDefinition) -> Result<(), CatalogError> {
    let invalid = |field| CatalogError::InvalidDefinition {
        target: target.id.clone(),
        field,
    };
    if !std::iter::once(&target.id)
        .chain(&target.aliases)
        .chain([&target.product_id, &target.variant])
        .all(|name| identifier(name))
    {
        return Err(invalid("identifier"));
    }
    let mut scopes = std::collections::BTreeSet::new();
    for definition in &target.scopes {
        if !scopes.insert(definition.scope) {
            return Err(invalid("duplicate scope"));
        }
        let platforms: std::collections::BTreeSet<_> = definition.platforms.iter().collect();
        if platforms.is_empty() || platforms.len() != definition.platforms.len() {
            return Err(invalid("platforms"));
        }
        if matches!(
            definition.status,
            SupportStatus::ImportOnly | SupportStatus::Unsupported
        ) && definition.preferred_write.is_some()
        {
            return Err(invalid("non-writable status"));
        }
        if matches!(
            definition.status,
            SupportStatus::ConfigurationRequired | SupportStatus::Unsupported
        ) && definition
            .limitations
            .iter()
            .all(|value| value.trim().is_empty())
        {
            return Err(invalid("status explanation"));
        }
        if definition.scope != Scope::Project
            && definition
                .discovery
                .iter()
                .any(|location| location.traversal == DiscoveryTraversal::AncestorsToRepository)
        {
            return Err(invalid("discovery traversal scope"));
        }
        for rule in definition
            .preferred_write
            .iter()
            .chain(definition.discovery.iter().map(|d| &d.location))
        {
            let fallback = match &rule.path.base {
                PathBase::Environment { variable, fallback } => {
                    if variable.is_empty()
                        || !variable.bytes().enumerate().all(|(i, c)| {
                            c == b'_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit())
                        })
                    {
                        return Err(invalid("environment variable"));
                    }
                    fallback.as_ref().map(|path| (&path.base, &path.components))
                }
                _ => None,
            };
            if !components(&rule.path.components)
                || fallback.is_some_and(|(_, path)| !components(path))
            {
                return Err(invalid("path components"));
            }
            let base = match &rule.path.base {
                PathBase::Home => Some(ContextBase::Home),
                PathBase::Project => Some(ContextBase::Project),
                PathBase::ApplicationWorkspace => Some(ContextBase::ApplicationWorkspace),
                PathBase::Environment { .. } => fallback.map(|(base, _)| *base),
            };
            let expected = match definition.scope {
                Scope::Project => ContextBase::Project,
                Scope::User => ContextBase::Home,
                Scope::ApplicationWorkspace => ContextBase::ApplicationWorkspace,
            };
            if base.is_some_and(|base| base != expected) {
                return Err(invalid("scope/path base mismatch"));
            }
            let evidence = &rule.evidence;
            let reviewed_required = matches!(
                definition.status,
                SupportStatus::Documented | SupportStatus::ConfigurationRequired
            );
            if (reviewed_required && evidence.documentation != DocumentationStatus::Reviewed)
                || (evidence.documentation == DocumentationStatus::Reviewed
                    && evidence.source.is_none())
            {
                return Err(invalid("missing reviewed documentation"));
            }
            if let Some(source) = &evidence.source {
                if !documentation_url(&source.url)
                    || !date(&source.accessed)
                    || source.section.trim().is_empty()
                    || source.applicability.trim().is_empty()
                {
                    return Err(invalid("documentation provenance"));
                }
            }
            if evidence
                .resolver_tests
                .iter()
                .any(|test| test.trim().is_empty())
                || evidence.discovery_tests.iter().any(|test| {
                    test.host_version.trim().is_empty()
                        || test.evidence.trim().is_empty()
                        || !definition.platforms.contains(&test.platform)
                })
            {
                return Err(invalid("test evidence"));
            }
        }
    }
    Ok(())
}
