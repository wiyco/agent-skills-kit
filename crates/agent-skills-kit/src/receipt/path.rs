use super::ReceiptError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Lossless native path data, never a grant to read, replace or delete a path.
///
/// UTF-8 uses compact text; non-Unicode paths retain native units. Foreign
/// encodings remain readable as data and cannot silently become local paths.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(
    tag = "format",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[non_exhaustive]
pub enum NativePath {
    /// Unix UTF-8 bytes, preserved without normalization.
    UnixUtf8(String),
    /// Non-UTF-8 Unix bytes; valid UTF-8 belongs in UnixUtf8 instead.
    UnixBytes(Vec<u8>),
    /// Windows Unicode path, preserved without normalization.
    WindowsUtf8(String),
    /// Windows UTF-16 containing unpaired surrogates.
    WindowsWide(Vec<u16>),
}
impl NativePath {
    pub(super) fn windows(&self) -> bool {
        matches!(self, Self::WindowsUtf8(_) | Self::WindowsWide(_))
    }
    pub(super) fn relative_entry(&self) -> bool {
        let units: Vec<u32> = match self {
            Self::UnixUtf8(text) | Self::WindowsUtf8(text) => {
                text.encode_utf16().map(u32::from).collect()
            }
            Self::UnixBytes(bytes) => bytes.iter().copied().map(u32::from).collect(),
            Self::WindowsWide(units) => units.iter().copied().map(u32::from).collect(),
        };
        let windows = self.windows();
        if windows && units.contains(&u32::from(b':')) {
            return false;
        }
        units
            .split(|unit| *unit == u32::from(b'/') || (windows && *unit == u32::from(b'\\')))
            .all(|part| {
                !part.is_empty()
                    && part != [u32::from(b'.')]
                    && part != [u32::from(b'.'), u32::from(b'.')]
            })
    }
    pub(super) fn manifest(&self) -> bool {
        matches!(self,Self::UnixUtf8(text) | Self::WindowsUtf8(text) if text=="SKILL.md")
    }
    /// Encode the local native spelling without filesystem access.
    pub fn from_native(path: &Path) -> Result<Self, ReceiptError> {
        #[cfg(unix)]
        let result = {
            use std::os::unix::ffi::OsStrExt;
            match path.to_str() {
                Some(text) => Self::UnixUtf8(text.to_owned()),
                None => Self::UnixBytes(path.as_os_str().as_bytes().to_vec()),
            }
        };
        #[cfg(windows)]
        let result = {
            use std::os::windows::ffi::OsStrExt;
            match path.to_str() {
                Some(text) => Self::WindowsUtf8(text.to_owned()),
                None => Self::WindowsWide(path.as_os_str().encode_wide().collect()),
            }
        };
        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            return Err(ReceiptError::UnsupportedPathPlatform);
        }
        #[cfg(any(unix, windows))]
        {
            result.validate(32768)?;
            Ok(result)
        }
    }
    /// Decode only the local native family, without touching the filesystem.
    pub fn to_native(&self) -> Result<PathBuf, ReceiptError> {
        self.validate(32768)?;
        match self {
            #[cfg(unix)]
            Self::UnixUtf8(text) => Ok(PathBuf::from(text)),
            #[cfg(unix)]
            Self::UnixBytes(bytes) => {
                use std::{ffi::OsString, os::unix::ffi::OsStringExt};
                Ok(PathBuf::from(OsString::from_vec(bytes.clone())))
            }
            #[cfg(windows)]
            Self::WindowsUtf8(text) => Ok(PathBuf::from(text)),
            #[cfg(windows)]
            Self::WindowsWide(units) => {
                use std::{ffi::OsString, os::windows::ffi::OsStringExt};
                Ok(PathBuf::from(OsString::from_wide(units)))
            }
            _ => Err(ReceiptError::UnsupportedPathPlatform),
        }
    }
    pub(super) fn units(&self) -> usize {
        match self {
            Self::UnixUtf8(text) => text.len(),
            Self::UnixBytes(bytes) => bytes.len(),
            Self::WindowsUtf8(text) => text.encode_utf16().count(),
            Self::WindowsWide(units) => units.len(),
        }
    }
    pub(super) fn validate(&self, max: usize) -> Result<(), ReceiptError> {
        if self.units() == 0 {
            return Err(ReceiptError::InvalidDocument("native path units"));
        }
        if self.units() > max {
            return Err(ReceiptError::Limit("native path units"));
        }
        let valid = match self {
            Self::UnixUtf8(text) | Self::WindowsUtf8(text) => !text.contains('\0'),
            Self::UnixBytes(bytes) => !bytes.contains(&0) && std::str::from_utf8(bytes).is_err(),
            Self::WindowsWide(units) => !units.contains(&0) && String::from_utf16(units).is_err(),
        };
        if valid {
            Ok(())
        } else {
            Err(ReceiptError::InvalidDocument(
                "noncanonical or NUL path encoding",
            ))
        }
    }
}
