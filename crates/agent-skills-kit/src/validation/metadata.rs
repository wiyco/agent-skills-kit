use std::{collections::BTreeMap, error::Error, fmt};

use crate::catalog::{ReservedSkillName, TargetDefinition};

use super::{MetadataValue, MetadataValueKind, yaml};

/// One-based diagnostic position in the original UTF-8 SKILL.md.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Location {
    /// Source line, including the opening frontmatter delimiter.
    pub line: usize,
    /// One-based parser column; not a terminal display width.
    pub column: usize,
}

/// Typed metadata diagnostic; text is untrusted and needs terminal escaping.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    /// Specific validation failure.
    pub kind: DiagnosticKind,
    /// Best available source location. Missing fields point to the header start.
    pub location: Location,
}

impl Diagnostic {
    pub(super) fn header(kind: DiagnosticKind) -> Self {
        Self {
            kind,
            location: Location { line: 2, column: 1 },
        }
    }
}
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "skill metadata error at {}:{}: {:?}",
            self.location.line, self.location.column, self.kind
        )
    }
}
impl Error for Diagnostic {}

/// Metadata errors are separate from filesystem and host-discovery failures.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DiagnosticKind {
    /// Resource limits must be positive and within the documented hard ceilings.
    InvalidLimits,
    /// A named document/parser budget was exceeded.
    Limit(&'static str),
    /// Input is not UTF-8.
    Encoding,
    /// A UTF-8 BOM is rejected explicitly; source bytes are never rewritten.
    Bom,
    /// An opening or closing standalone `---` delimiter is missing.
    Delimiter,
    /// YAML parser failure, with no command interpretation.
    Yaml(String),
    /// Unsupported bounded YAML feature.
    UnsupportedYaml(&'static str),
    /// The document root must be a string-keyed mapping.
    MappingRequired,
    /// A required metadata field is absent.
    MissingField(&'static str),
    /// A known field has the wrong YAML data type.
    FieldType(String),
    /// A duplicate mapping key was rejected rather than overwritten.
    DuplicateField(String),
    /// Known string field length is outside the specification limits.
    FieldLength(&'static str),
    /// Skill name syntax or portable filename safety failed.
    InvalidName,
    /// Metadata name must equal the caller-supplied source directory name.
    DirectoryNameMismatch,
}

/// Configurable finite parser budgets, checked before allocating a parsed tree.
#[derive(Clone, Copy, Debug)]
pub struct MetadataLimits {
    /// Total SKILL.md bytes; default 1 MiB, hard ceiling 16 MiB.
    pub max_document_bytes: usize,
    /// Frontmatter bytes; default 64 KiB, hard ceiling 1 MiB.
    pub max_frontmatter_bytes: usize,
    /// Scalar/collection nodes, including keys; default 4096, ceiling 65536.
    pub max_nodes: usize,
    /// Nested collections including the root; default 16, ceiling 64.
    pub max_depth: usize,
}
impl Default for MetadataLimits {
    fn default() -> Self {
        Self {
            max_document_bytes: 1024 * 1024,
            max_frontmatter_bytes: 64 * 1024,
            max_nodes: 4096,
            max_depth: 16,
        }
    }
}
impl MetadataLimits {
    fn validate(self) -> Result<(), Diagnostic> {
        if self.max_document_bytes == 0
            || self.max_document_bytes > 16 * 1024 * 1024
            || self.max_frontmatter_bytes == 0
            || self.max_frontmatter_bytes > 1024 * 1024
            || self.max_nodes == 0
            || self.max_nodes > 65536
            || self.max_depth == 0
            || self.max_depth > 64
        {
            Err(Diagnostic::header(DiagnosticKind::InvalidLimits))
        } else {
            Ok(())
        }
    }
}

/// Validated inert metadata retaining the exact source and all extension data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkillMetadata {
    source: Vec<u8>,
    body_offset: usize,
    fields: BTreeMap<String, MetadataValue>,
    name: String,
    description: String,
}
impl SkillMetadata {
    /// Exact accepted SKILL.md bytes, including extensions, body and line endings.
    pub fn source_bytes(&self) -> &[u8] {
        &self.source
    }
    /// Exact body bytes after the closing delimiter, not interpreted or executed.
    pub fn body_bytes(&self) -> &[u8] {
        &self.source[self.body_offset..]
    }
    /// All frontmatter fields, including unknown extensions and their locations.
    pub fn fields(&self) -> &BTreeMap<String, MetadataValue> {
        &self.fields
    }
    /// General-spec validated name, matching the source directory.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Required nonempty specification description.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Check additional modeled core-host constraints without host invocation.
    ///
    /// This does not prove loading, configuration enablement or payload safety.
    pub fn validate_for_target(
        &self,
        target: &TargetDefinition,
    ) -> Result<(), TargetConstraintError> {
        if let Some(reservation) = target.reserved_skill_name(&self.name) {
            return Err(TargetConstraintError::ReservedName(reservation));
        }
        if target.requires_ascii_skill_name()
            && !self
                .name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(TargetConstraintError::AsciiNameRequired);
        }
        Ok(())
    }
}

/// A host constraint failure, separate from general specification diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TargetConstraintError {
    /// A documented core-host name reservation applies.
    ReservedName(ReservedSkillName),
    /// OpenCode's documented name grammar is ASCII, unlike the general Unicode rule.
    AsciiNameRequired,
}
impl fmt::Display for TargetConstraintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "host skill constraint failed: {self:?}")
    }
}
impl Error for TargetConstraintError {}

/// Validate SKILL.md bytes and the explicit source directory name as inert data.
///
/// Accepts LF/CRLF without rewriting bytes. Rejects BOM, aliases, anchors, merge
/// keys, collection tags, complex/non-string mapping keys and duplicate fields.
/// Unknown scalar tags/extensions are retained as data, never instantiated.
/// All limits are checked; no filesystem or process-environment access occurs.
///
/// ```
/// use agent_skills_kit::validation::{parse_metadata, MetadataLimits};
/// let bytes = b"---\nname: example\ndescription: An inert example\n---\nBody\n";
/// let metadata = parse_metadata(bytes, "example", MetadataLimits::default())?;
/// assert_eq!(metadata.name(), "example");
/// assert_eq!(metadata.source_bytes(), bytes);
/// # Ok::<(), agent_skills_kit::validation::Diagnostic>(())
/// ```
pub fn parse_metadata(
    bytes: &[u8],
    directory_name: &str,
    limits: MetadataLimits,
) -> Result<SkillMetadata, Diagnostic> {
    limits.validate()?;
    if bytes.len() > limits.max_document_bytes {
        return Err(Diagnostic::header(DiagnosticKind::Limit("SKILL.md bytes")));
    }
    let input = std::str::from_utf8(bytes).map_err(|error| {
        let prefix = std::str::from_utf8(&bytes[..error.valid_up_to()]).unwrap_or("");
        Diagnostic {
            kind: DiagnosticKind::Encoding,
            location: Location {
                line: prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
                column: prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1,
            },
        }
    })?;
    if input.starts_with('\u{feff}') {
        return Err(Diagnostic {
            kind: DiagnosticKind::Bom,
            location: Location { line: 1, column: 1 },
        });
    }
    let first_end = input
        .find('\n')
        .ok_or_else(|| Diagnostic::header(DiagnosticKind::Delimiter))?
        + 1;
    if line_content(&input[..first_end]) != "---" {
        return Err(Diagnostic::header(DiagnosticKind::Delimiter));
    }
    let mut end = first_end;
    let mut header_end = None;
    for line in input[first_end..].split_inclusive('\n') {
        if line_content(line) == "---" {
            header_end = Some(end);
            end += line.len();
            break;
        }
        end += line.len();
        if end - first_end > limits.max_frontmatter_bytes {
            return Err(Diagnostic::header(DiagnosticKind::Limit(
                "frontmatter bytes",
            )));
        }
    }
    let header_end = header_end.ok_or_else(|| Diagnostic::header(DiagnosticKind::Delimiter))?;
    if header_end - first_end > limits.max_frontmatter_bytes {
        return Err(Diagnostic::header(DiagnosticKind::Limit(
            "frontmatter bytes",
        )));
    }
    let root = yaml::parse(&input[first_end..header_end], &limits)?;
    let MetadataValueKind::Mapping(fields) = root.value else {
        return Err(Diagnostic {
            kind: DiagnosticKind::MappingRequired,
            location: root.location,
        });
    };
    let name = string_field(&fields, "name", true)?.expect("required field checked");
    let description = string_field(&fields, "description", true)?.expect("required field checked");
    if !valid_name(name) {
        return Err(field_error(&fields, "name", DiagnosticKind::InvalidName));
    }
    if name != directory_name {
        return Err(field_error(
            &fields,
            "name",
            DiagnosticKind::DirectoryNameMismatch,
        ));
    }
    if description.trim().is_empty() || description.chars().count() > 1024 {
        return Err(field_error(
            &fields,
            "description",
            DiagnosticKind::FieldLength("description"),
        ));
    }
    if let Some(compatibility) = string_field(&fields, "compatibility", false)? {
        if compatibility.trim().is_empty() || compatibility.chars().count() > 500 {
            return Err(field_error(
                &fields,
                "compatibility",
                DiagnosticKind::FieldLength("compatibility"),
            ));
        }
    }
    string_field(&fields, "license", false)?;
    string_field(&fields, "allowed-tools", false)?;
    if let Some(metadata) = fields.get("metadata") {
        let MetadataValueKind::Mapping(values) = &metadata.value else {
            return Err(field_error(
                &fields,
                "metadata",
                DiagnosticKind::FieldType("metadata".into()),
            ));
        };
        for value in values.values() {
            if value.string().is_none() {
                return Err(Diagnostic {
                    kind: DiagnosticKind::FieldType("metadata value".into()),
                    location: value.location,
                });
            }
        }
    }
    Ok(SkillMetadata {
        name: name.to_owned(),
        description: description.to_owned(),
        source: bytes.to_vec(),
        body_offset: end,
        fields,
    })
}

fn field_error(
    fields: &BTreeMap<String, MetadataValue>,
    field: &'static str,
    kind: DiagnosticKind,
) -> Diagnostic {
    Diagnostic {
        kind,
        location: fields
            .get(field)
            .map_or(Location { line: 2, column: 1 }, |value| value.location),
    }
}
fn string_field<'a>(
    fields: &'a BTreeMap<String, MetadataValue>,
    field: &'static str,
    required: bool,
) -> Result<Option<&'a str>, Diagnostic> {
    match fields.get(field) {
        Some(value) => value
            .string()
            .map(Some)
            .ok_or_else(|| field_error(fields, field, DiagnosticKind::FieldType(field.into()))),
        None if required => Err(Diagnostic::header(DiagnosticKind::MissingField(field))),
        None => Ok(None),
    }
}
fn valid_name(name: &str) -> bool {
    let length = name.chars().count();
    if !(1..=64).contains(&length)
        || name.starts_with('-')
        || name.ends_with('-')
        || name.contains("--")
        || !name.chars().all(|ch| {
            ch == '-' || (ch.is_alphanumeric() && ch.to_lowercase().eq(std::iter::once(ch)))
        })
    {
        return false;
    }
    let upper = name.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return false;
    }
    if let Some(suffix) = upper
        .strip_prefix("COM")
        .or_else(|| upper.strip_prefix("LPT"))
    {
        if matches!(
            suffix,
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        ) {
            return false;
        }
    }
    true
}

fn line_content(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}
