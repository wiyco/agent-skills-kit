use caseless::Caseless;
use std::{collections::BTreeMap, ffi::OsStr, path::Path};
use unicode_normalization::UnicodeNormalization;

use super::plan::PlanError;
use crate::validation::SourceEntry;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum ComponentKey {
    Unicode(String),
    Native(Vec<u8>),
}

// A conservative portable collision policy, not a claim to emulate every
// filesystem collation/version. No input filename is rewritten.
pub(super) fn component_key(name: &OsStr) -> ComponentKey {
    match name.to_str() {
        Some(text) => ComponentKey::Unicode(text.nfd().default_case_fold().nfd().collect()),
        None => ComponentKey::Native(name.as_encoded_bytes().to_ascii_lowercase()),
    }
}
pub(super) fn path_key(path: &Path) -> Vec<ComponentKey> {
    path.iter().map(component_key).collect()
}

pub(super) fn reject_payload_collisions(entries: &[SourceEntry]) -> Result<(), PlanError> {
    let mut seen = BTreeMap::new();
    for entry in entries {
        let key = path_key(&entry.relative_path);
        if let Some(previous) = seen.insert(key, &entry.relative_path) {
            return Err(PlanError::PortableNameCollision {
                first: previous.clone(),
                second: entry.relative_path.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_caseless_keys_include_expansion_and_decomposition() {
        for (a, b) in [
            ("README", "readme"),
            ("café", "cafe\u{301}"),
            ("straße", "strasse"),
            ("σ", "ς"),
        ] {
            assert_eq!(component_key(OsStr::new(a)), component_key(OsStr::new(b)));
        }
        assert_ne!(path_key(Path::new("a/b")), path_key(Path::new("ab")));
        assert_ne!(
            component_key(OsStr::new("alpha")),
            component_key(OsStr::new("beta"))
        );
    }
}
