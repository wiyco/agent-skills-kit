//! Read-only destination observations for subsequent install-plan construction.
//!
//! Observations are preconditions, not write authorization or atomic snapshots.

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
mod names;
mod namespace;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) mod native;
mod plan;

pub use namespace::*;
pub use plan::*;

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) fn ambiguous_names(
    values: &[std::ffi::OsString],
) -> std::collections::BTreeSet<std::ffi::OsString> {
    let mut groups = std::collections::BTreeMap::<_, Vec<_>>::new();
    for value in values {
        groups
            .entry(names::component_key(value))
            .or_default()
            .push(value);
    }
    groups
        .into_values()
        .filter(|group| group.len() > 1)
        .flatten()
        .cloned()
        .collect()
}
