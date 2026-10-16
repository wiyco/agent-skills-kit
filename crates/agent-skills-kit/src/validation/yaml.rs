use std::collections::BTreeMap;

use saphyr_parser::{Event, Parser, ScalarStyle};

use super::{Diagnostic, DiagnosticKind, Location, MetadataLimits};

/// An uninterpreted YAML scalar. No custom tag is instantiated or executed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScalarValue {
    /// Decoded scalar text; exact original formatting remains in source bytes.
    pub text: String,
    /// Whether the scalar used YAML plain style rather than quotes/block style.
    pub plain: bool,
    /// Explicit YAML tag, if present. Only inert scalar tags are retained.
    pub tag: Option<String>,
}

impl ScalarValue {
    pub(super) fn string(&self) -> Option<&str> {
        match self.tag.as_deref() {
            Some("tag:yaml.org,2002:str" | "!") => Some(&self.text),
            Some(_) => None,
            None if !self.plain || !non_string_scalar(&self.text) => Some(&self.text),
            None => None,
        }
    }
}

// Resolve only whether an untagged plain scalar is a YAML core string. Unknown
// extension scalars remain uninterpreted, including their spelling and tag.
fn non_string_scalar(text: &str) -> bool {
    if matches!(
        text,
        "" | "~"
            | "null"
            | "Null"
            | "NULL"
            | "true"
            | "True"
            | "TRUE"
            | "false"
            | "False"
            | "FALSE"
    ) {
        return true;
    }
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    if matches!(unsigned, ".inf" | ".Inf" | ".INF") || matches!(text, ".nan" | ".NaN" | ".NAN") {
        return true;
    }
    if let Some(digits) = text.strip_prefix("0x") {
        return !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_hexdigit());
    }
    if let Some(digits) = text.strip_prefix("0o") {
        return !digits.is_empty() && digits.bytes().all(|byte| matches!(byte, b'0'..=b'7'));
    }
    let mut exponent_parts = unsigned.split(['e', 'E']);
    let mantissa = exponent_parts.next().unwrap_or("");
    if let Some(exponent) = exponent_parts.next() {
        if exponent_parts.next().is_some()
            || !digits(exponent.strip_prefix(['+', '-']).unwrap_or(exponent))
        {
            return false;
        }
    }
    if let Some((whole, fraction)) = mantissa.split_once('.') {
        (whole.is_empty() || digits(whole))
            && (fraction.is_empty() || digits(fraction))
            && !(whole.is_empty() && fraction.is_empty())
    } else {
        digits(mantissa)
    }
}

fn digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// A location-bearing inert YAML value, preserving unknown extension structure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataValue {
    /// One-based position in the original SKILL.md.
    pub location: Location,
    /// Scalar, sequence or string-keyed mapping data.
    pub value: MetadataValueKind,
}

/// YAML data accepted by the bounded metadata reader.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MetadataValueKind {
    /// Raw scalar data; no automatic object or command construction.
    Scalar(ScalarValue),
    /// Ordered YAML sequence.
    Sequence(Vec<MetadataValue>),
    /// String-keyed mapping, sorted deterministically; duplicates are rejected.
    Mapping(BTreeMap<String, MetadataValue>),
}

impl MetadataValue {
    pub(super) fn string(&self) -> Option<&str> {
        match &self.value {
            MetadataValueKind::Scalar(scalar) => scalar.string(),
            _ => None,
        }
    }
}

pub(super) fn parse(input: &str, limits: &MetadataLimits) -> Result<MetadataValue, Diagnostic> {
    let mut events = Vec::new();
    let mut depth = 0usize;
    let mut nodes = 0usize;
    let mut documents = 0usize;
    for event in Parser::new_from_str(input) {
        let (event, span) = event.map_err(|error| Diagnostic {
            kind: DiagnosticKind::Yaml(error.info().to_owned()),
            // Parser lines are one-based, columns are zero-based in 0.2.2.
            location: Location {
                line: error.marker().line() + 1,
                column: error.marker().col() + 1,
            },
        })?;
        let location = Location {
            line: span.start.line() + 1,
            column: span.start.col() + 1,
        };
        match &event {
            Event::Alias(_) => {
                return Err(Diagnostic {
                    kind: DiagnosticKind::UnsupportedYaml("aliases"),
                    location,
                });
            }
            Event::Scalar(_, _, anchor, tag)
            | Event::MappingStart(anchor, tag)
            | Event::SequenceStart(anchor, tag) => {
                if *anchor != 0 {
                    return Err(Diagnostic {
                        kind: DiagnosticKind::UnsupportedYaml("anchors"),
                        location,
                    });
                }
                if tag.is_some() && !matches!(event, Event::Scalar(..)) {
                    return Err(Diagnostic {
                        kind: DiagnosticKind::UnsupportedYaml("collection tags"),
                        location,
                    });
                }
                nodes += 1;
                if nodes > limits.max_nodes {
                    return Err(Diagnostic {
                        kind: DiagnosticKind::Limit("YAML nodes"),
                        location,
                    });
                }
                if matches!(event, Event::MappingStart(..) | Event::SequenceStart(..)) {
                    depth += 1;
                    if depth > limits.max_depth {
                        return Err(Diagnostic {
                            kind: DiagnosticKind::Limit("YAML depth"),
                            location,
                        });
                    }
                }
                events.push((event, location));
            }
            Event::MappingEnd | Event::SequenceEnd => {
                depth = depth.saturating_sub(1);
                events.push((event, location));
            }
            Event::DocumentStart(_) => {
                documents += 1;
                if documents > 1 {
                    return Err(Diagnostic {
                        kind: DiagnosticKind::UnsupportedYaml("multiple documents"),
                        location,
                    });
                }
            }
            _ => {}
        }
    }
    let mut events = events.into_iter().peekable();
    let value = node(&mut events)?;
    if events.next().is_some() {
        return Err(Diagnostic::header(DiagnosticKind::Yaml(
            "extra YAML value".to_owned(),
        )));
    }
    Ok(value)
}

fn node<'a>(
    events: &mut std::iter::Peekable<impl Iterator<Item = (Event<'a>, Location)>>,
) -> Result<MetadataValue, Diagnostic> {
    let (event, location) = events
        .next()
        .ok_or_else(|| Diagnostic::header(DiagnosticKind::Yaml("missing YAML value".to_owned())))?;
    let value = match event {
        Event::Scalar(text, style, _, tag) => MetadataValueKind::Scalar(ScalarValue {
            text: text.into_owned(),
            plain: style == ScalarStyle::Plain,
            tag: tag.map(|tag| format!("{}{}", tag.handle, tag.suffix)),
        }),
        Event::SequenceStart(..) => {
            let mut values = Vec::new();
            while !matches!(events.peek(), Some((Event::SequenceEnd, _))) {
                values.push(node(events)?);
            }
            events.next();
            MetadataValueKind::Sequence(values)
        }
        Event::MappingStart(..) => {
            let mut values = BTreeMap::new();
            while !matches!(events.peek(), Some((Event::MappingEnd, _))) {
                let key = node(events)?;
                let name = key.string().ok_or(Diagnostic {
                    kind: DiagnosticKind::UnsupportedYaml("non-string mapping keys"),
                    location: key.location,
                })?;
                if name == "<<" {
                    return Err(Diagnostic {
                        kind: DiagnosticKind::UnsupportedYaml("merge keys"),
                        location: key.location,
                    });
                }
                let name = name.to_owned();
                if values.contains_key(&name) {
                    return Err(Diagnostic {
                        kind: DiagnosticKind::DuplicateField(name),
                        location: key.location,
                    });
                }
                values.insert(name, node(events)?);
            }
            events.next();
            MetadataValueKind::Mapping(values)
        }
        _ => {
            return Err(Diagnostic {
                kind: DiagnosticKind::Yaml("unexpected YAML event".to_owned()),
                location,
            });
        }
    };
    Ok(MetadataValue { location, value })
}
