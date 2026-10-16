use agent_skills_kit::catalog::{Catalog, CatalogError, Scope, SupportStatus};
use serde_json::{Value, json};

// Original synthetic metadata, not a host path or a claim of official evidence.
fn target(id: &str) -> Value {
    json!({
        "id": id, "aliases": [], "product_id": id, "variant": "cli",
        "limitations": [], "scopes": [{
            "scope": "project", "status": "documented", "platforms": ["linux"],
            "limitations": [], "discovery": [], "preferred_write": {
                "path": {"base": {"kind":"project"}, "components":["skills"]},
                "evidence": {
                    "documentation":"reviewed", "resolver_tests":[],
                    "discovery_tests":[], "limitations":[], "source": {
                        "url":"https://example.invalid/synthetic-schema-fixture",
                        "section":"Synthetic test", "accessed":"2026-10-16",
                        "applicability":"Synthetic fixture only"
                    }
                }
            }
        }]
    })
}

fn parse(targets: Vec<Value>) -> Result<Catalog, CatalogError> {
    let document = json!({"schema_version":1,"targets":targets});
    Catalog::from_json(&serde_json::to_vec(&document).unwrap())
}

#[test]
fn deterministic_order_alias_lookup_and_round_trip() {
    let mut a = target("alpha");
    a["aliases"] = json!(["first"]);
    let first = parse(vec![target("zeta"), a.clone()]).unwrap();
    let second = parse(vec![a, target("zeta")]).unwrap();
    assert_eq!(first.document(), second.document());
    assert_eq!(first.get("first").unwrap().id, "alpha");
    assert!(first.get("ALPHA").is_none());
    let encoded = serde_json::to_vec(&first.document()).unwrap();
    assert_eq!(
        Catalog::from_json(&encoded).unwrap().document(),
        first.document()
    );
}

#[test]
fn rejects_duplicate_ids_aliases_and_variants() {
    assert!(matches!(
        parse(vec![target("same"), target("same")]),
        Err(CatalogError::DuplicateName(_))
    ));
    let mut a = target("alpha");
    a["aliases"] = json!(["beta"]);
    assert!(matches!(
        parse(vec![a, target("beta")]),
        Err(CatalogError::DuplicateName(_))
    ));
    let mut a = target("alpha");
    a["aliases"] = json!(["alias", "alias"]);
    assert!(matches!(
        parse(vec![a]),
        Err(CatalogError::DuplicateName(_))
    ));
    let mut b = target("beta");
    b["product_id"] = json!("alpha");
    assert!(matches!(
        parse(vec![target("alpha"), b]),
        Err(CatalogError::DuplicateVariant(_, _))
    ));
}

#[test]
fn variants_are_distinct_and_ids_are_not_normalized() {
    let a = target("alpha");
    let mut b = target("alpha-ide");
    b["product_id"] = json!("alpha");
    b["variant"] = json!("ide");
    assert_eq!(parse(vec![a, b]).unwrap().targets().len(), 2);
    for id in [
        "",
        "../escape",
        "Uppercase",
        "-alpha",
        "a--b",
        "a/b",
        "a\\b",
        "a\0b",
    ] {
        assert!(parse(vec![target(id)]).is_err(), "accepted ID {id:?}");
    }
}

#[test]
fn rejects_unknown_versions_scopes_fields_and_truncated_json() {
    assert!(matches!(
        Catalog::from_json(br#"{"schema_version":2,"targets":[]}"#),
        Err(CatalogError::UnsupportedVersion(2))
    ));
    let mut a = target("alpha");
    a["scopes"][0]["scope"] = json!("admin");
    assert!(matches!(parse(vec![a]), Err(CatalogError::Parse(_))));
    let mut a = target("alpha");
    a["mystery"] = json!(true);
    assert!(matches!(parse(vec![a]), Err(CatalogError::Parse(_))));
    assert!(matches!(
        Catalog::from_json(b"{"),
        Err(CatalogError::Parse(_))
    ));
}

#[test]
fn missing_write_path_and_unverified_evidence_never_become_defaults() {
    let mut a = target("alpha");
    a["scopes"][0]
        .as_object_mut()
        .unwrap()
        .remove("preferred_write");
    let catalog = parse(vec![a]).unwrap();
    assert!(
        catalog.get("alpha").unwrap().scopes[0]
            .preferred_write
            .is_none()
    );
    assert_eq!(
        catalog.get("alpha").unwrap().scopes[0].scope,
        Scope::Project
    );
    let mut a = target("alpha");
    a["scopes"][0]["status"] = json!("unverified");
    a["scopes"][0]["preferred_write"]["evidence"]["source"] = Value::Null;
    a["scopes"][0]["preferred_write"]["evidence"]["documentation"] = json!("unverified");
    let catalog = parse(vec![a]).unwrap();
    assert_eq!(
        catalog.get("alpha").unwrap().scopes[0].status,
        SupportStatus::Unverified
    );
}

#[test]
fn rejects_unsafe_path_components_and_mismatched_scope_bases() {
    for component in [
        "",
        ".",
        "..",
        "/absolute",
        "a/b",
        "a\\b",
        "C:",
        "a\0b",
        "a\nb",
    ] {
        let mut a = target("alpha");
        a["scopes"][0]["preferred_write"]["path"]["components"] = json!([component]);
        assert!(parse(vec![a]).is_err(), "accepted component {component:?}");
    }
    let mut a = target("alpha");
    a["scopes"][0]["preferred_write"]["path"]["base"] = json!({"kind":"home"});
    assert!(parse(vec![a]).is_err());
}

#[test]
fn environment_overrides_have_explicit_validated_fallbacks() {
    let mut a = target("alpha");
    a["scopes"][0]["scope"] = json!("user");
    a["scopes"][0]["preferred_write"]["path"]["base"] = json!({
        "kind":"environment", "variable":"XDG_CONFIG_HOME",
        "fallback":{"base":"home","components":[".config"]}
    });
    assert!(parse(vec![a.clone()]).is_ok());
    for variable in ["", "9INVALID", "$(command)", "A=B", "A\0B"] {
        let mut b = a.clone();
        b["scopes"][0]["preferred_write"]["path"]["base"]["variable"] = json!(variable);
        assert!(parse(vec![b]).is_err());
    }
    a["scopes"][0]["preferred_write"]["path"]["base"]["fallback"]["components"] = json!([".."]);
    assert!(parse(vec![a]).is_err());
}

#[test]
fn requires_reviewed_provenance_and_valid_calendar_dates() {
    for (field, value) in [
        ("url", "http://example.invalid"),
        ("url", "https://"),
        ("url", "https://user@example.invalid/docs"),
        ("url", "https://bad host/docs"),
        ("url", "https://:443/docs"),
        ("url", "https://example.invalid:bad/docs"),
        ("url", "https://-bad.invalid/docs"),
        ("url", "https://example.invalid/\0"),
        ("section", " "),
        ("applicability", ""),
        ("accessed", "2025-02-29"),
        ("accessed", "2026-13-01"),
        ("accessed", "2026-+1-01"),
        ("accessed", "0000-01-01"),
    ] {
        let mut a = target("alpha");
        a["scopes"][0]["preferred_write"]["evidence"]["source"][field] = json!(value);
        assert!(parse(vec![a]).is_err(), "accepted {field}={value:?}");
    }
    let mut a = target("alpha");
    a["scopes"][0]["preferred_write"]["evidence"]["source"]["accessed"] = json!("2024-02-29");
    assert!(parse(vec![a.clone()]).is_ok());
    a["scopes"][0]["preferred_write"]["evidence"]["source"] = Value::Null;
    assert!(parse(vec![a]).is_err());
    let mut a = target("alpha");
    a["scopes"][0]["preferred_write"]["evidence"]["documentation"] = json!("conflicting");
    assert!(parse(vec![a]).is_err());
}

#[test]
fn validates_scope_platform_status_and_separate_test_evidence() {
    let mut a = target("alpha");
    a["scopes"][0]["platforms"] = json!([]);
    assert!(parse(vec![a]).is_err());
    let mut a = target("alpha");
    a["scopes"][0]["platforms"] = json!(["linux", "linux"]);
    assert!(parse(vec![a]).is_err());
    let mut a = target("alpha");
    let duplicate = a["scopes"][0].clone();
    a["scopes"].as_array_mut().unwrap().push(duplicate);
    assert!(parse(vec![a]).is_err());
    for status in ["import_only", "unsupported"] {
        let mut a = target("alpha");
        a["scopes"][0]["status"] = json!(status);
        assert!(parse(vec![a]).is_err());
    }
    let mut a = target("alpha");
    a["scopes"][0]["preferred_write"]["evidence"]["discovery_tests"] = json!([{
        "platform":"windows", "host_version":"1", "evidence":"synthetic"
    }]);
    assert!(parse(vec![a]).is_err());
}

#[test]
fn parser_limit_and_errors_are_explicit() {
    assert!(matches!(
        Catalog::from_json(&vec![b' '; 2 * 1024 * 1024 + 1]),
        Err(CatalogError::InputTooLarge)
    ));
    let error = Catalog::from_json(b"{").unwrap_err();
    assert!(std::error::Error::source(&error).is_some());
    assert!(error.to_string().contains("invalid catalog JSON"));
}

#[test]
fn conditional_and_read_only_statuses_remain_explicit() {
    let mut a = target("alpha");
    a["scopes"][0]["status"] = json!("configuration_required");
    assert!(parse(vec![a.clone()]).is_err());
    a["scopes"][0]["limitations"] = json!(["Synthetic configuration required"]);
    assert!(parse(vec![a]).is_ok());
    for status in ["unsupported", "import_only"] {
        let mut a = target("alpha");
        a["scopes"][0]["status"] = json!(status);
        a["scopes"][0]["preferred_write"] = Value::Null;
        a["scopes"][0]["limitations"] = json!(["No local destination"]);
        assert!(
            parse(vec![a]).unwrap().get("alpha").unwrap().scopes[0]
                .preferred_write
                .is_none()
        );
    }
}

#[test]
fn fallback_absence_and_discovery_traversal_are_not_guessed() {
    let mut a = target("alpha");
    a["scopes"][0]["preferred_write"]["path"]["base"] = json!({
        "kind":"environment", "variable":"EXAMPLE_ROOT", "fallback":null
    });
    let catalog = parse(vec![a.clone()]).unwrap();
    let rule = catalog.get("alpha").unwrap().scopes[0]
        .preferred_write
        .as_ref()
        .unwrap();
    assert!(matches!(
        rule.path.base,
        agent_skills_kit::catalog::PathBase::Environment { fallback: None, .. }
    ));
    a["scopes"][0]["scope"] = json!("user");
    a["scopes"][0]["discovery"] = json!([{
        "location":a["scopes"][0]["preferred_write"].clone(),
        "traversal":"ancestors_to_repository"
    }]);
    assert!(parse(vec![a]).is_err());
}
