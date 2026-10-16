use agent_skills_kit::{
    catalog::{Platform, Scope},
    receipt::*,
    resolver::native_platform,
};
use std::path::{Path, PathBuf};

fn path(value: &str) -> NativePath {
    NativePath::from_native(Path::new(value)).unwrap()
}
fn document() -> ReceiptDocument {
    let root = if cfg!(windows) {
        r"C:\fixture\source\sample"
    } else {
        "/fixture/source/sample"
    };
    let destination = if cfg!(windows) {
        r"C:\fixture\target\sample"
    } else {
        "/fixture/target/sample"
    };
    ReceiptDocument {
        schema_version: RECEIPT_SCHEMA_VERSION,
        operation_id: OperationId::new("original-fixture-001").unwrap(),
        platform: native_platform().unwrap(),
        scope: Scope::Project,
        digest_algorithm: DigestAlgorithm::Blake3,
        sources: vec![ReceiptSource {
            root: path(root),
            name: "sample".into(),
            root_mode: 0o755,
            entries: vec![ReceiptEntry {
                path: path("SKILL.md"),
                kind: ReceiptEntryKind::File,
                mode: 0o644,
                bytes: 5,
                digest: Some(ContentDigest::from_bytes(&[0x42; 32])),
            }],
        }],
        targets: vec!["codex".into()],
        destinations: vec![ReceiptDestination {
            path: Some(path(destination)),
            attributions: vec![ReceiptAttribution {
                source: 0,
                target: 0,
            }],
            status: ReceiptStatus::Created,
            failure: None,
        }],
        preflight_failure: None,
        cleanup_diagnostics: vec![],
    }
}
fn encoded() -> Vec<u8> {
    Receipt::new(document(), ReceiptLimits::default())
        .unwrap()
        .to_json(ReceiptLimits::default())
        .unwrap()
}
fn rejects(mutator: impl FnOnce(&mut ReceiptDocument)) {
    let mut doc = document();
    mutator(&mut doc);
    assert!(Receipt::new(doc, ReceiptLimits::default()).is_err());
}

#[test]
fn original_observational_document_round_trips_stably() {
    let doc = document();
    let receipt = Receipt::new(doc.clone(), ReceiptLimits::default()).unwrap();
    let bytes = receipt.to_json(ReceiptLimits::default()).unwrap();
    let parsed = Receipt::from_json(&bytes, ReceiptLimits::default()).unwrap();
    assert_eq!(parsed.document(), &doc);
    assert_eq!(parsed.to_json(ReceiptLimits::default()).unwrap(), bytes);
    assert_eq!(
        parsed.document().operation_id.as_str(),
        "original-fixture-001"
    );
    assert_eq!(
        parsed.document().sources[0].entries[0]
            .digest
            .as_ref()
            .unwrap()
            .as_str(),
        "42".repeat(32)
    );
    assert_eq!(
        parsed.document().destinations[0].status,
        ReceiptStatus::Created
    );
    // Pure data only: the original synthetic paths above are never opened.
}

#[test]
fn unknown_versions_fields_enums_duplicates_and_corrupt_json_fail() {
    let limits = ReceiptLimits::default();
    assert_eq!(
        Receipt::from_json(br#"{"schema_version":2,"future_field":true}"#, limits).unwrap_err(),
        ReceiptError::UnsupportedVersion(2)
    );
    for bytes in [b"{".as_slice(), b"[]", b"null", b"{}", b"\xff"] {
        assert!(Receipt::from_json(bytes, limits).is_err());
    }
    let original = String::from_utf8(encoded()).unwrap();
    let duplicate = format!("{{\"schema_version\":1,{}", &original[1..]);
    assert!(Receipt::from_json(duplicate.as_bytes(), limits).is_err());
    for mutate in [
        |value: &mut serde_json::Value| {
            value["unknown"] = true.into();
        },
        |value: &mut serde_json::Value| {
            value["sources"][0]["unknown"] = true.into();
        },
        |value: &mut serde_json::Value| {
            value["destinations"][0]["status"] = "force_replaced".into();
        },
        |value: &mut serde_json::Value| {
            value["digest_algorithm"] = "guessed".into();
        },
        |value: &mut serde_json::Value| {
            value["sources"][0]["entries"][0]["digest"] = "short".into();
        },
        |value: &mut serde_json::Value| {
            value["operation_id"] = "invalid operation".into();
        },
    ] {
        let mut value: serde_json::Value = serde_json::from_slice(&encoded()).unwrap();
        mutate(&mut value);
        assert!(Receipt::from_json(&serde_json::to_vec(&value).unwrap(), limits).is_err());
    }
}

#[test]
fn exact_byte_and_structural_budgets_are_enforced_before_success() {
    let bytes = encoded();
    let limits = ReceiptLimits {
        max_document_bytes: bytes.len(),
        ..ReceiptLimits::default()
    };
    let receipt = Receipt::from_json(&bytes, limits).unwrap();
    assert_eq!(receipt.to_json(limits).unwrap(), bytes);
    let smaller = ReceiptLimits {
        max_document_bytes: bytes.len() - 1,
        ..limits
    };
    assert_eq!(
        Receipt::from_json(&bytes, smaller).unwrap_err(),
        ReceiptError::Limit("JSON bytes")
    );
    assert_eq!(
        receipt.to_json(smaller).unwrap_err(),
        ReceiptError::Limit("JSON bytes")
    );
    for limits in [
        ReceiptLimits {
            max_document_bytes: 0,
            ..limits
        },
        ReceiptLimits {
            max_entries: 65537,
            ..limits
        },
        ReceiptLimits {
            max_path_units: 32769,
            ..limits
        },
        ReceiptLimits {
            max_total_path_units: 0,
            ..limits
        },
        ReceiptLimits {
            max_diagnostics: 131073,
            ..limits
        },
    ] {
        assert_eq!(
            Receipt::from_json(&bytes, limits).unwrap_err(),
            ReceiptError::InvalidLimits
        );
    }
    assert!(
        Receipt::new(
            document(),
            ReceiptLimits {
                max_path_units: 4,
                ..limits
            }
        )
        .is_err()
    );
    assert_eq!(
        Receipt::new(
            document(),
            ReceiptLimits {
                max_total_path_units: 1,
                ..limits
            }
        )
        .unwrap_err(),
        ReceiptError::Limit("path units")
    );
}

#[test]
fn invalid_inventory_references_and_status_relationships_are_rejected() {
    rejects(|doc| doc.sources.clear());
    rejects(|doc| doc.targets.clear());
    rejects(|doc| doc.destinations.clear());
    rejects(|doc| doc.sources.push(doc.sources[0].clone()));
    rejects(|doc| doc.targets.push("codex".into()));
    rejects(|doc| {
        let duplicate = doc.sources[0].entries[0].clone();
        doc.sources[0].entries.push(duplicate);
    });
    rejects(|doc| doc.sources[0].entries.clear());
    rejects(|doc| doc.sources[0].root_mode = 0o4755);
    rejects(|doc| doc.sources[0].entries[0].mode = 0o666);
    rejects(|doc| doc.sources[0].entries[0].digest = None);
    rejects(|doc| doc.sources[0].entries[0].kind = ReceiptEntryKind::Directory);
    rejects(|doc| doc.sources[0].entries[0].path = path("../SKILL.md"));
    rejects(|doc| doc.destinations[0].attributions[0].source = 1);
    rejects(|doc| doc.destinations[0].attributions.clear());
    rejects(|doc| doc.destinations.push(doc.destinations[0].clone()));
    rejects(|doc| doc.destinations[0].status = ReceiptStatus::Failed);
    rejects(|doc| doc.destinations[0].path = None);
    rejects(|doc| {
        doc.preflight_failure = Some(ReceiptDiagnostic {
            code: ReceiptFailureCode::InapplicablePlan,
            path: None,
            operation: None,
            os_error: None,
        })
    });
}

#[test]
fn unchanged_records_preexisting_content_without_creation_or_cleanup_authority() {
    let mut doc = document();
    doc.destinations[0].status = ReceiptStatus::Unchanged;
    let receipt = Receipt::new(doc, ReceiptLimits::default()).unwrap();
    let bytes = receipt.to_json(ReceiptLimits::default()).unwrap();
    let parsed = Receipt::from_json(&bytes, ReceiptLimits::default()).unwrap();
    assert_eq!(
        parsed.document().destinations[0].status,
        ReceiptStatus::Unchanged
    );
    assert!(parsed.document().cleanup_diagnostics.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["destinations"][0]["status"], "unchanged");
    // No receipt API takes a record as permission to mutate the filesystem.
}

#[test]
fn path_encodings_are_canonical_lossless_and_platform_explicit() {
    let native = if cfg!(windows) {
        PathBuf::from(r"C:\fixture\café\file")
    } else {
        PathBuf::from("/fixture/café/file")
    };
    let encoded = NativePath::from_native(&native).unwrap();
    assert_eq!(encoded.to_native().unwrap(), native);
    for invalid in [
        NativePath::UnixUtf8("a\0b".into()),
        NativePath::UnixBytes(b"valid utf8".to_vec()),
        NativePath::WindowsWide(vec![b'a' as u16]),
        NativePath::WindowsUtf8(String::new()),
    ] {
        assert!(invalid.to_native().is_err());
    }
    #[cfg(unix)]
    {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let path = PathBuf::from(OsString::from_vec(b"/fixture/raw-\xff".to_vec()));
        let encoded = NativePath::from_native(&path).unwrap();
        assert!(matches!(&encoded, NativePath::UnixBytes(_)));
        assert_eq!(encoded.to_native().unwrap(), path);
        assert_eq!(
            NativePath::WindowsWide(vec![0xd800])
                .to_native()
                .unwrap_err(),
            ReceiptError::UnsupportedPathPlatform
        );
    }
    #[cfg(windows)]
    {
        use std::{
            ffi::OsString,
            os::windows::ffi::{OsStrExt, OsStringExt},
        };
        let units = [b'C' as u16, b':' as u16, b'\\' as u16, 0xd800];
        let path = PathBuf::from(OsString::from_wide(&units));
        let encoded = NativePath::from_native(&path).unwrap();
        assert!(matches!(&encoded, NativePath::WindowsWide(_)));
        assert_eq!(
            encoded
                .to_native()
                .unwrap()
                .as_os_str()
                .encode_wide()
                .collect::<Vec<_>>(),
            units
        );
        assert_eq!(
            NativePath::UnixBytes(vec![0xff]).to_native().unwrap_err(),
            ReceiptError::UnsupportedPathPlatform
        );
    }
    let mut doc = document();
    doc.platform = if cfg!(windows) {
        Platform::Linux
    } else {
        Platform::Windows
    };
    assert!(Receipt::new(doc, ReceiptLimits::default()).is_err());
}

#[test]
fn imported_foreign_windows_surrogates_remain_data_on_unix() {
    let mut doc = document();
    doc.platform = Platform::Windows;
    doc.sources[0].root =
        NativePath::WindowsWide(vec![b'C' as u16, b':' as u16, b'\\' as u16, 0xd800]);
    doc.sources[0].entries[0].path = NativePath::WindowsUtf8("SKILL.md".into());
    doc.destinations[0].path = Some(NativePath::WindowsUtf8(r"C:\fixture\target".into()));
    let receipt = Receipt::new(doc, ReceiptLimits::default()).unwrap();
    let bytes = receipt.to_json(ReceiptLimits::default()).unwrap();
    assert_eq!(
        Receipt::from_json(&bytes, ReceiptLimits::default()).unwrap(),
        receipt
    );
}

#[test]
fn identifier_and_source_count_boundaries_are_explicit() {
    for length in [1, 128] {
        assert!(OperationId::new("a".repeat(length)).is_ok());
    }
    for value in [
        String::new(),
        "a".repeat(129),
        "has space".into(),
        "../id/escape".into(),
        "line\nbreak".into(),
    ] {
        assert!(OperationId::new(value.clone()).is_err());
        assert!(
            serde_json::from_str::<OperationId>(&serde_json::to_string(&value).unwrap()).is_err()
        );
    }
    assert!(
        serde_json::from_str::<ContentDigest>(&serde_json::to_string(&"AA".repeat(32)).unwrap())
            .is_err()
    );
    let base = document();
    let mut doc = base.clone();
    doc.sources.clear();
    doc.destinations.clear();
    for index in 0..128 {
        let mut source = base.sources[0].clone();
        source.root = path(&if cfg!(windows) {
            format!(r"C:\fixture\source-{index}\sample")
        } else {
            format!("/fixture/source-{index}/sample")
        });
        doc.sources.push(source);
        let mut destination = base.destinations[0].clone();
        destination.attributions[0].source = index;
        destination.path = Some(path(&if cfg!(windows) {
            format!(r"C:\fixture\target-{index}\sample")
        } else {
            format!("/fixture/target-{index}/sample")
        }));
        doc.destinations.push(destination);
    }
    assert!(
        Receipt::new(
            doc.clone(),
            ReceiptLimits {
                max_entries: 128,
                ..ReceiptLimits::default()
            }
        )
        .is_ok()
    );
    assert_eq!(
        Receipt::new(
            doc.clone(),
            ReceiptLimits {
                max_entries: 127,
                ..ReceiptLimits::default()
            }
        )
        .unwrap_err(),
        ReceiptError::Limit("payload entries")
    );
    doc.sources.push(base.sources[0].clone());
    assert_eq!(
        Receipt::new(doc, ReceiptLimits::default()).unwrap_err(),
        ReceiptError::Limit("source/target/destination count")
    );
}
