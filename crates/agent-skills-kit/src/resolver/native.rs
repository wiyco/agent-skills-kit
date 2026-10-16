use std::{
    ffi::OsStr,
    path::{Component, Path},
};

use super::{PathIssue, Platform};

/// The compilation target, never a process-global environment lookup.
pub(super) const fn platform() -> Option<Platform> {
    if cfg!(target_os = "linux") {
        Some(Platform::Linux)
    } else if cfg!(target_os = "macos") {
        Some(Platform::Macos)
    } else if cfg!(target_os = "windows") {
        Some(Platform::Windows)
    } else {
        None
    }
}

pub(super) fn validate_root(path: &Path) -> Result<(), PathIssue> {
    if path.as_os_str().is_empty() {
        return Err(PathIssue::Empty);
    }
    if !path.is_absolute() {
        return Err(PathIssue::Relative);
    }
    if path.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(PathIssue::Nul);
    }
    for component in path.components() {
        match component {
            Component::ParentDir => return Err(PathIssue::ParentComponent),
            Component::Normal(name) if !valid_component(name) => {
                return Err(PathIssue::InvalidComponent);
            }
            #[cfg(windows)]
            Component::Prefix(prefix) => {
                use std::path::Prefix;
                if matches!(prefix.kind(), Prefix::DeviceNS(_) | Prefix::Verbatim(_)) {
                    return Err(PathIssue::DeviceNamespace);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn valid_component(value: &OsStr) -> bool {
    if value.is_empty() || value.as_encoded_bytes().contains(&0) {
        return false;
    }
    let mut parts = Path::new(value).components();
    if !matches!(parts.next(), Some(Component::Normal(_))) || parts.next().is_some() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let units: Vec<u16> = value.encode_wide().collect();
        if units
            .iter()
            .any(|unit| *unit < 32 || b"<>:\"|?*".iter().any(|c| *unit == u16::from(*c)))
            || matches!(units.last(), Some(32 | 46))
        {
            return false;
        }
        let mut base: Vec<u16> = units
            .iter()
            .copied()
            .take_while(|unit| *unit != 46)
            .map(|unit| match unit {
                97..=122 => unit - 32,
                0x00b9 => 49,
                0x00b2 => 50,
                0x00b3 => 51,
                _ => unit,
            })
            .collect();
        // Conservatively reject device aliases with basename padding as well.
        while base.last() == Some(&32) {
            base.pop();
        }
        let ascii = |text: &str| base.iter().copied().eq(text.bytes().map(u16::from));
        if ["CON", "PRN", "AUX", "NUL"].iter().any(|name| ascii(name))
            || (base.len() == 4
                && (base.starts_with(&[67, 79, 77]) || base.starts_with(&[76, 80, 84]))
                && (49..=57).contains(&base[3]))
        {
            return false;
        }
    }
    true
}
