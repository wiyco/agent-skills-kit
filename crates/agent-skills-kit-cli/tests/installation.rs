mod common;
use agent_skills_kit::{
    catalog::{Catalog, Scope},
    receipt::{Receipt, ReceiptLimits},
    resolver::{InstallContext, native_platform, resolve},
};
use common::{Fixture, cli, json};
use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
};
fn context(fixture: &Fixture) -> InstallContext {
    let mut context = InstallContext::new(native_platform().unwrap());
    context.user_home = Some(fixture.0.join("home"));
    context.project_root = Some(fixture.0.join("project"));
    fs::create_dir(context.user_home.as_ref().unwrap()).unwrap();
    fs::create_dir(context.project_root.as_ref().unwrap()).unwrap();
    context
}
fn args(source: &Path, context: &InstallContext, scope: &str) -> Vec<OsString> {
    vec![
        "install".into(),
        source.as_os_str().to_owned(),
        "--scope".into(),
        scope.into(),
        "--user-home".into(),
        context.user_home.as_ref().unwrap().as_os_str().to_owned(),
        "--project-root".into(),
        context
            .project_root
            .as_ref()
            .unwrap()
            .as_os_str()
            .to_owned(),
        "--target".into(),
        "codex".into(),
        "--target".into(),
        "codex-cli".into(),
        "--target".into(),
        "claude-code".into(),
        "--target".into(),
        "opencode".into(),
        "--json".into(),
    ]
}
fn tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(root: &Path, path: &Path, result: &mut Vec<(PathBuf, Vec<u8>)>) {
        let metadata = fs::symlink_metadata(path).unwrap();
        result.push((
            path.strip_prefix(root).unwrap().into(),
            if metadata.is_file() {
                fs::read(path).unwrap()
            } else {
                vec![]
            },
        ));
        if metadata.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                walk(root, &entry.unwrap().path(), result);
            }
        }
    }
    let mut result = vec![];
    walk(root, root, &mut result);
    result.sort();
    result
}
#[test]
fn scripts_install_both_scopes_using_library_destinations_and_versioned_receipts() {
    for (scope, label) in [(Scope::User, "user"), (Scope::Project, "project")] {
        let fixture = Fixture::new();
        let source = fixture.source(OsStr::new("source café"));
        let context = context(&fixture);
        let mut command = args(&source, &context, label);
        command.push("--yes".into());
        let output = cli(&command);
        assert!(output.status.success(), "{:?}", output.stderr);
        let value = json(&output);
        let data = &value["data"];
        assert_eq!(data["apply_attempted"], true);
        assert_eq!(data["report"]["successful"], true);
        assert_eq!(data["report"]["outcomes"].as_array().unwrap().len(), 3);
        Receipt::from_json(
            &serde_json::to_vec(&data["receipt"]).unwrap(),
            ReceiptLimits::default(),
        )
        .unwrap();
        for target in ["codex", "claude-code", "opencode"] {
            let destination = resolve(&Catalog::bundled().unwrap(), target, scope, &context)
                .unwrap()
                .preferred_write
                .path
                .join("sample");
            assert_eq!(
                fs::read(destination.join("SKILL.md")).unwrap(),
                fs::read(source.join("SKILL.md")).unwrap()
            );
            assert_eq!(
                fs::read(destination.join("resource.ps1")).unwrap(),
                b"inert resource; never executed"
            );
            assert!(destination.join("empty").is_dir());
        }
        let output = cli(&command);
        assert!(output.status.success());
        let repeated = json(&output);
        assert!(
            repeated["data"]["report"]["outcomes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["status"] == "unchanged")
        );
        Receipt::from_json(
            &serde_json::to_vec(&repeated["data"]["receipt"]).unwrap(),
            ReceiptLimits::default(),
        )
        .unwrap();
    }
}
#[test]
fn unapproved_install_returns_its_plan_without_reading_stdin_or_writing() {
    let fixture = Fixture::new();
    let source = fixture.source(OsStr::new("source"));
    let context = context(&fixture);
    let before = tree(&fixture.0);
    let output = cli(args(&source, &context, "project"));
    assert_eq!(output.status.code(), Some(3));
    let value = json(&output);
    assert_eq!(value["status"], "approval_required");
    assert_eq!(value["data"]["approval_required"], true);
    assert_eq!(value["data"]["apply_attempted"], false);
    assert_eq!(
        value["data"]["plan"]["destinations"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(before, tree(&fixture.0));
}
#[test]
fn dry_run_with_or_without_yes_matches_plan_and_never_creates_staging() {
    let fixture = Fixture::new();
    let source = fixture.source(OsStr::new("source"));
    let context = context(&fixture);
    let command = args(&source, &context, "user");
    let mut plan = command.clone();
    plan[0] = "plan".into();
    let preview = json(&cli(plan));
    for yes in [false, true] {
        let mut dry = command.clone();
        dry.push("--dry-run".into());
        if yes {
            dry.push("--yes".into());
        }
        let before = tree(&fixture.0);
        let output = cli(dry);
        assert!(output.status.success());
        let value = json(&output);
        assert_eq!(value["data"]["plan"], preview["data"]);
        assert_eq!(value["data"]["dry_run"], true);
        assert_eq!(value["data"]["apply_attempted"], false);
        assert!(value["data"]["receipt"].is_null());
        assert_eq!(before, tree(&fixture.0));
    }
}
#[test]
fn yes_never_overwrites_manual_edits_or_bypasses_unknown_options() {
    let fixture = Fixture::new();
    let source = fixture.source(OsStr::new("source"));
    let context = context(&fixture);
    let mut command = args(&source, &context, "project");
    command.push("--yes".into());
    assert!(cli(&command).status.success());
    let destination = resolve(
        &Catalog::bundled().unwrap(),
        "codex",
        Scope::Project,
        &context,
    )
    .unwrap()
    .preferred_write
    .path
    .join("sample/resource.ps1");
    fs::write(&destination, b"manual edit").unwrap();
    let before = tree(&fixture.0);
    let output = cli(&command);
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output);
    assert_eq!(value["status"], "blocked");
    assert_eq!(value["data"]["apply_attempted"], false);
    assert_eq!(before, tree(&fixture.0));
    assert_eq!(fs::read(destination).unwrap(), b"manual edit");
    for extra in ["--force", "--yes"] {
        let mut invalid = command.clone();
        invalid.push(extra.into());
        let output = cli(invalid);
        assert_eq!(output.status.code(), Some(2));
        json(&output);
        assert_eq!(before, tree(&fixture.0));
    }
}
#[test]
fn invalid_targets_scopes_and_source_fail_before_writes_even_with_approval() {
    let fixture = Fixture::new();
    let source = fixture.source(OsStr::new("source"));
    let context = context(&fixture);
    let base = args(&source, &context, "project");
    for (option, replacement) in [("--target", "unknown"), ("--scope", "workspace")] {
        let mut command = base.clone();
        let position = command.iter().position(|value| value == option).unwrap();
        command[position + 1] = replacement.into();
        command.push("--yes".into());
        let before = tree(&fixture.0);
        let output = cli(command);
        assert_eq!(output.status.code(), Some(2));
        json(&output);
        assert_eq!(before, tree(&fixture.0));
    }
    fs::write(source.join("SKILL.md"), b"invalid").unwrap();
    let mut command = base;
    command.push("--yes".into());
    let before = tree(&fixture.0);
    let output = cli(command);
    assert_eq!(output.status.code(), Some(1));
    json(&output);
    assert_eq!(before, tree(&fixture.0));
}

#[test]
fn unsafe_source_links_fail_before_apply_and_preserve_outside_sentinels() {
    let fixture = Fixture::new();
    let source = fixture.source(OsStr::new("source"));
    let context = context(&fixture);
    let outside = fixture.0.join("outside sentinel");
    fs::write(&outside, b"untouched").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, source.join("unsafe link")).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&outside, source.join("unsafe link"))
        .expect("required Windows unsafe-link fixture privilege; no passing skip");
    let mut command = args(&source, &context, "project");
    command.push("--yes".into());
    let output = cli(command);
    assert_eq!(output.status.code(), Some(1));
    json(&output);
    assert_eq!(fs::read(outside).unwrap(), b"untouched");
    assert_eq!(
        fs::read_dir(context.user_home.as_ref().unwrap())
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        fs::read_dir(context.project_root.as_ref().unwrap())
            .unwrap()
            .count(),
        0
    );
    assert!(
        fs::symlink_metadata(source.join("unsafe link"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
