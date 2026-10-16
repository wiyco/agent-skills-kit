use agent_skills_kit::{catalog::Catalog, validation::*};

fn parse(header: &str, name: &str) -> Result<SkillMetadata, Diagnostic> {
    parse_metadata(
        format!("---\n{header}\n---\nInert body\n").as_bytes(),
        name,
        MetadataLimits::default(),
    )
}
fn named(name: &str) -> Result<SkillMetadata, Diagnostic> {
    parse(
        &format!("name: '{name}'\ndescription: Original inert fixture"),
        name,
    )
}

#[test]
fn minimal_metadata_retains_exact_bytes_body_and_extensions() {
    let bytes = b"---\r\nname: example\r\ndescription: |\r\n  Inert fixture\r\n  for tests\r\nmetadata:\r\n  version: '1.0'\r\nlicense: MIT\r\ncompatibility: Needs a local directory\r\nallowed-tools: Read\r\ncustom:\r\n  list: [one, 2, true]\r\n  opaque: !do-not-run '$(touch forbidden)'\r\n---\r\nNever execute: `touch forbidden`\r\n";
    let result = parse_metadata(bytes, "example", MetadataLimits::default()).unwrap();
    assert_eq!(result.source_bytes(), bytes);
    assert_eq!(result.name(), "example");
    assert_eq!(result.description(), "Inert fixture\nfor tests\n");
    assert_eq!(result.body_bytes(), b"Never execute: `touch forbidden`\r\n");
    assert_eq!(
        result.fields()["name"].location,
        Location { line: 2, column: 7 }
    );
    let MetadataValueKind::Mapping(custom) = &result.fields()["custom"].value else {
        panic!("extension mapping lost")
    };
    let MetadataValueKind::Scalar(opaque) = &custom["opaque"].value else {
        panic!("extension tag lost")
    };
    assert_eq!(opaque.text, "$(touch forbidden)");
    assert_eq!(opaque.tag.as_deref(), Some("!do-not-run"));
    assert!(
        matches!(&custom["list"].value, MetadataValueKind::Sequence(values) if values.len() == 3)
    );
}

#[test]
fn required_fields_types_and_useful_locations() {
    assert_eq!(
        parse("description: fixture", "example").unwrap_err().kind,
        DiagnosticKind::MissingField("name")
    );
    assert_eq!(
        parse("name: example", "example").unwrap_err().kind,
        DiagnosticKind::MissingField("description")
    );
    let error = parse("name: example\ndescription: [wrong, type]", "example").unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::FieldType("description".into()));
    assert_eq!(error.location.line, 3);
    for value in [
        "true", "FALSE", "null", "~", "123", "1.5", "1e20", "0xFF", "0o77", ".inf",
    ] {
        assert!(
            matches!(
                parse(&format!("name: example\ndescription: {value}"), "example")
                    .unwrap_err()
                    .kind,
                DiagnosticKind::FieldType(_)
            ),
            "coerced non-string {value}"
        );
    }
    for text in ["+.nan", "+0xFF", "0x", "1_000", "1e", "1.2.3", "inf", "NaN"] {
        assert_eq!(
            parse(&format!("name: example\ndescription: {text}"), "example")
                .unwrap()
                .description(),
            text
        );
    }
    assert_eq!(
        parse("name: example\ndescription: ! 123", "example")
            .unwrap()
            .description(),
        "123"
    );
    assert_eq!(
        parse("name: example\ndescription: !!str 123", "example")
            .unwrap()
            .description(),
        "123"
    );
    for header in [
        "name: example\ndescription: ok\nmetadata: []",
        "name: example\ndescription: ok\nmetadata: {version: 1}",
        "name: example\ndescription: ok\ncompatibility: false",
        "name: example\ndescription: ok\nlicense: []",
        "name: example\ndescription: ok\nallowed-tools: []",
    ] {
        assert!(matches!(
            parse(header, "example").unwrap_err().kind,
            DiagnosticKind::FieldType(_)
        ));
    }
}

#[test]
fn malformed_yaml_duplicates_and_unsupported_constructs_are_rejected() {
    let error = parse("name: example\ndescription: [unterminated", "example").unwrap_err();
    assert!(matches!(error.kind, DiagnosticKind::Yaml(_)));
    assert!(error.location.line >= 3);
    assert_eq!(
        parse(
            "name: example\nname: other\ndescription: fixture",
            "example"
        )
        .unwrap_err()
        .kind,
        DiagnosticKind::DuplicateField("name".into())
    );
    assert!(matches!(
        parse(
            "name: example\ndescription: ok\ncustom: {x: one, x: two}",
            "example"
        )
        .unwrap_err()
        .kind,
        DiagnosticKind::DuplicateField(_)
    ));
    for header in [
        "name: &n example\ndescription: *n",
        "name: example\ndescription: ok\ncustom: {<<: other}",
        "name: example\ndescription: ok\ncustom: !object [one]",
        "name: example\ndescription: ok\ncustom: {[one]: two}",
        "name: example\ndescription: ok\ncustom: {1: value}",
    ] {
        assert!(matches!(
            parse(header, "example").unwrap_err().kind,
            DiagnosticKind::UnsupportedYaml(_)
        ));
    }
    assert_eq!(
        parse("[one, two]", "example").unwrap_err().kind,
        DiagnosticKind::MappingRequired
    );
}

#[test]
fn delimiter_encoding_and_bom_policy_is_explicit() {
    for bytes in [
        &b"name: example\n"[..],
        b"---\nname: example\n",
        b" ---\nname: example\n---\n",
        b"---\r\r\nname: example\n---\n",
    ] {
        assert_eq!(
            parse_metadata(bytes, "example", MetadataLimits::default())
                .unwrap_err()
                .kind,
            DiagnosticKind::Delimiter
        );
    }
    assert_eq!(
        parse_metadata(
            b"\xef\xbb\xbf---\n---\n",
            "example",
            MetadataLimits::default()
        )
        .unwrap_err()
        .kind,
        DiagnosticKind::Bom
    );
    assert_eq!(
        parse_metadata(b"---\n\xff\n---\n", "example", MetadataLimits::default())
            .unwrap_err()
            .kind,
        DiagnosticKind::Encoding
    );
    let bytes = b"---\nname: example\ndescription: ok\n---";
    assert!(
        parse_metadata(bytes, "example", MetadataLimits::default())
            .unwrap()
            .body_bytes()
            .is_empty()
    );
}

#[test]
fn names_and_portable_device_reservations_cannot_inject_paths() {
    for name in [
        "",
        "../escape",
        "/absolute",
        "a/b",
        "a\\b",
        "C:drive",
        "a:b",
        ".",
        "..",
        "UPPER",
        "-edge",
        "edge-",
        "two--hyphens",
        "a b",
        "zero\u{200b}width",
        "con",
        "prn",
        "aux",
        "nul",
        "com1",
        "lpt9",
        "com¹",
        "lpt³",
        "ǅ",
    ] {
        assert_eq!(
            named(name).unwrap_err().kind,
            DiagnosticKind::InvalidName,
            "accepted {name:?}"
        );
    }
    for name in [
        "a",
        "pdf-processing",
        "skill123",
        "日本語",
        "café",
        "com0",
        "com10",
        "null-example",
    ] {
        assert!(named(name).is_ok(), "rejected {name}");
    }
    assert!(named(&"a".repeat(64)).is_ok());
    assert_eq!(
        named(&"a".repeat(65)).unwrap_err().kind,
        DiagnosticKind::InvalidName
    );
    assert_eq!(
        parse("name: example\ndescription: ok", "other")
            .unwrap_err()
            .kind,
        DiagnosticKind::DirectoryNameMismatch
    );
}

#[test]
fn optional_and_required_field_lengths_have_boundaries() {
    for field in ["description", "compatibility"] {
        let max = if field == "description" { 1024 } else { 500 };
        for (value, accepted) in [
            (" ".to_owned(), false),
            ("é".repeat(max), true),
            ("é".repeat(max + 1), false),
        ] {
            let header = if field == "description" {
                format!("name: example\ndescription: '{value}'")
            } else {
                format!("name: example\ndescription: ok\ncompatibility: '{value}'")
            };
            assert_eq!(parse(&header, "example").is_ok(), accepted);
        }
    }
}

#[test]
fn budgets_reject_size_depth_and_node_excess_at_boundaries() {
    let bytes = b"---\nname: example\ndescription: ok\n---\n";
    let mut limits = MetadataLimits {
        max_document_bytes: bytes.len(),
        max_frontmatter_bytes: b"name: example\ndescription: ok\n".len(),
        max_nodes: 5,
        max_depth: 1,
    };
    assert!(parse_metadata(bytes, "example", limits).is_ok());
    limits.max_document_bytes -= 1;
    assert_eq!(
        parse_metadata(bytes, "example", limits).unwrap_err().kind,
        DiagnosticKind::Limit("SKILL.md bytes")
    );
    limits.max_document_bytes += 1;
    limits.max_frontmatter_bytes -= 1;
    assert_eq!(
        parse_metadata(bytes, "example", limits).unwrap_err().kind,
        DiagnosticKind::Limit("frontmatter bytes")
    );
    limits.max_frontmatter_bytes += 1;
    limits.max_nodes -= 1;
    assert_eq!(
        parse_metadata(bytes, "example", limits).unwrap_err().kind,
        DiagnosticKind::Limit("YAML nodes")
    );
    let nested = format!(
        "---\nname: example\ndescription: ok\ncustom: {}x{}\n---\n",
        "[".repeat(17),
        "]".repeat(17)
    );
    assert_eq!(
        parse_metadata(nested.as_bytes(), "example", MetadataLimits::default())
            .unwrap_err()
            .kind,
        DiagnosticKind::Limit("YAML depth")
    );
    for limits in [
        MetadataLimits {
            max_depth: 0,
            ..MetadataLimits::default()
        },
        MetadataLimits {
            max_depth: 65,
            ..MetadataLimits::default()
        },
        MetadataLimits {
            max_nodes: 65537,
            ..MetadataLimits::default()
        },
        MetadataLimits {
            max_frontmatter_bytes: 1024 * 1024 + 1,
            ..MetadataLimits::default()
        },
    ] {
        assert_eq!(
            parse_metadata(bytes, "example", limits).unwrap_err().kind,
            DiagnosticKind::InvalidLimits
        );
    }
}

#[test]
fn host_constraints_are_separate_from_general_specification() {
    let catalog = Catalog::bundled().unwrap();
    let synced = named("synced").unwrap();
    assert!(matches!(
        synced.validate_for_target(catalog.get("claude-code").unwrap()),
        Err(TargetConstraintError::ReservedName(_))
    ));
    assert!(
        synced
            .validate_for_target(catalog.get("codex").unwrap())
            .is_ok()
    );
    assert!(matches!(
        named("anthropic-skills")
            .unwrap()
            .validate_for_target(catalog.get("claude-code").unwrap()),
        Err(TargetConstraintError::ReservedName(_))
    ));
    let unicode = named("日本語").unwrap();
    assert_eq!(
        unicode.validate_for_target(catalog.get("opencode").unwrap()),
        Err(TargetConstraintError::AsciiNameRequired)
    );
    assert!(
        unicode
            .validate_for_target(catalog.get("codex").unwrap())
            .is_ok()
    );
}

#[test]
fn deterministic_adversarial_byte_inputs_do_not_panic() {
    // Original inert corpus, not a claim of exhaustive fuzzing.
    for byte in 0u8..=255 {
        let mut bytes = b"---\nname: example\ndescription: ".to_vec();
        bytes.extend(std::iter::repeat_n(byte, 32));
        bytes.extend_from_slice(b"\n---\n");
        let _ = parse_metadata(&bytes, "example", MetadataLimits::default());
    }
    for count in [1, 8, 64, 1024] {
        for token in ["[", "{", "&a ", "*a ", "!tag ", "? ", ": "] {
            let bytes = format!(
                "---\nname: example\ndescription: ok\nextension: {}\n---\n",
                token.repeat(count)
            );
            let _ = parse_metadata(bytes.as_bytes(), "example", MetadataLimits::default());
        }
    }
}

#[test]
fn hard_budget_ceilings_and_empty_documents_remain_bounded() {
    let base = MetadataLimits::default();
    for limits in [
        MetadataLimits {
            max_document_bytes: 0,
            ..base
        },
        MetadataLimits {
            max_document_bytes: 16 * 1024 * 1024 + 1,
            ..base
        },
        MetadataLimits {
            max_frontmatter_bytes: 0,
            ..base
        },
        MetadataLimits {
            max_nodes: 0,
            ..base
        },
    ] {
        assert_eq!(
            parse_metadata(b"", "example", limits).unwrap_err().kind,
            DiagnosticKind::InvalidLimits
        );
    }
    for depth in [63, 64] {
        let bytes = format!(
            "---\nname: example\ndescription: ok\ncustom: {}x{}\n---\n",
            "[".repeat(depth),
            "]".repeat(depth)
        );
        let limits = MetadataLimits {
            max_depth: 64,
            ..base
        };
        let result = parse_metadata(bytes.as_bytes(), "example", limits);
        if depth == 63 {
            assert!(result.is_ok());
        } else {
            assert_eq!(
                result.unwrap_err().kind,
                DiagnosticKind::Limit("YAML depth")
            );
        }
    }
    for bytes in [
        &b"---\n---\n"[..],
        b"---\n# comment only\n---\n",
        b"---\nname: example\ndescription: ok\n...\nsecond: document\n---\n",
    ] {
        assert!(parse_metadata(bytes, "example", base).is_err());
    }
    let error = parse_metadata(
        b"---\nname: example\ndescription: \xff\n---\n",
        "example",
        base,
    )
    .unwrap_err();
    assert_eq!(
        error.location,
        Location {
            line: 3,
            column: 14
        }
    );
}
