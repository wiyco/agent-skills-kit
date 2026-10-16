use caseless::Caseless;
use std::{collections::BTreeMap, ffi::OsStr, path::Path};
use unicode_normalization::UnicodeNormalization;

use super::plan::PlanError;
use crate::validation::SourceEntry;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum ComponentKey {
    Unicode(String),
    #[cfg(not(windows))]
    Native(Vec<u8>),
    #[cfg(windows)]
    Wide(Vec<WidePart>),
}
#[cfg(windows)]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum WidePart {
    Text(String),
    Surrogate(u16),
}

// A conservative portable collision policy, not a claim to emulate every
// filesystem collation/version. No input filename is rewritten.
pub(super) fn component_key(name: &OsStr) -> ComponentKey {
    match name.to_str() {
        Some(text) => ComponentKey::Unicode(text.nfd().default_case_fold().nfd().collect()),
        #[cfg(not(windows))]
        None => ComponentKey::Native(name.as_encoded_bytes().to_ascii_lowercase()),
        #[cfg(windows)]
        None => {
            use std::os::windows::ffi::OsStrExt;
            let mut parts = Vec::new();
            let mut text = String::new();
            for scalar in char::decode_utf16(name.encode_wide()) {
                match scalar {
                    Ok(scalar) => text.push(scalar),
                    Err(error) => {
                        if !text.is_empty() {
                            parts.push(WidePart::Text(
                                text.nfd().default_case_fold().nfd().collect(),
                            ));
                            text.clear();
                        }
                        parts.push(WidePart::Surrogate(error.unpaired_surrogate()));
                    }
                }
            }
            if !text.is_empty() {
                parts.push(WidePart::Text(
                    text.nfd().default_case_fold().nfd().collect(),
                ));
            }
            ComponentKey::Wide(parts)
        }
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
    #[cfg(windows)]
    #[test]
    fn non_unicode_keys_fold_scalar_segments_without_losing_surrogate_identity() {
        use std::{ffi::OsString, os::windows::ffi::OsStringExt};
        let name = |letter, surrogate| OsString::from_wide(&[b'r' as u16, letter, surrogate]);
        assert_eq!(
            component_key(&name(0xd6, 0xd800)),
            component_key(&name(0xf6, 0xd800))
        );
        assert_ne!(
            component_key(&name(0xd6, 0xd800)),
            component_key(&name(0xf6, 0xd801))
        );
        assert_ne!(
            component_key(&name(0xd6, 0xd800)),
            component_key(OsStr::new("rÖ�"))
        );
    }
}
