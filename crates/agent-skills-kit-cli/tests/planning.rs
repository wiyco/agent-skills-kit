mod common;
use agent_skills_kit::{
    catalog::{Catalog, Scope},
    executor::{ApplyOptions, apply_plan},
    planner::{InstallPlan, InstallRequest, PlanLimits, plan_install},
    receipt::NativePath,
    resolver::{InstallContext, native_platform},
    validation::{InventoryLimits, inventory_skill},
};
use common::{Fixture, cli, json};
use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
};
fn roots(fixture: &Fixture) -> (PathBuf, PathBuf) {
    let home = fixture.0.join("home with spaces");
    let project = fixture.0.join("project café");
    fs::create_dir(&home).unwrap();
    fs::create_dir(&project).unwrap();
    (home, project)
}
fn args(
    source: &Path,
    home: &Path,
    project: &Path,
    scope: &str,
    targets: &[&str],
) -> Vec<OsString> {
    let mut args = vec![
        "plan".into(),
        source.as_os_str().to_owned(),
        "--scope".into(),
        scope.into(),
        "--user-home".into(),
        home.as_os_str().to_owned(),
        "--project-root".into(),
        project.as_os_str().to_owned(),
        "--json".into(),
    ];
    for target in targets {
        args.push("--target".into());
        args.push((*target).into());
    }
    args
}
fn library(
    source: &Path,
    home: &Path,
    project: &Path,
    scope: Scope,
    targets: &[&str],
) -> InstallPlan {
    let mut context = InstallContext::new(native_platform().unwrap());
    context.user_home = Some(home.into());
    context.project_root = Some(project.into());
    let sources = [inventory_skill(source, InventoryLimits::default()).unwrap()];
    plan_install(
        &Catalog::bundled().unwrap(),
        InstallRequest {
            skills: &sources,
            target_ids: targets,
            scope,
            context: &context,
            limits: PlanLimits::default(),
        },
    )
    .unwrap()
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
fn both_scopes_match_library_paths_attributions_and_intended_writes_without_mutation() {
    for (scope, label) in [(Scope::User, "user"), (Scope::Project, "project")] {
        let fixture = Fixture::new();
        let source = fixture.source(OsStr::new("source"));
        let (home, project) = roots(&fixture);
        let targets = ["opencode", "codex-cli", "claude-code", "codex", "codex"];
        let expected = library(&source, &home, &project, scope, &targets);
        let before = tree(&fixture.0);
        let output = cli(args(&source, &home, &project, label, &targets));
        assert!(output.status.success(), "{:?}", output.stderr);
        let value = json(&output);
        assert_eq!(value["command"], "plan");
        assert_eq!(value["status"], "ok");
        let data = &value["data"];
        assert_eq!(data["applicable"], true);
        assert_eq!(
            data["destinations"].as_array().unwrap().len(),
            expected.destinations().len()
        );
        for (actual, row) in data["destinations"]
            .as_array()
            .unwrap()
            .iter()
            .zip(expected.destinations())
        {
            assert_eq!(
                actual["path"],
                serde_json::to_value(NativePath::from_native(row.path().unwrap()).unwrap())
                    .unwrap()
            );
            assert_eq!(actual["action"], "create");
            assert_eq!(
                actual["attributions"].as_array().unwrap().len(),
                row.attributions().len()
            );
        }
        assert_eq!(
            data["intended_writes"].as_array().unwrap().len(),
            expected.intended_writes().len()
        );
        for (actual, write) in data["intended_writes"]
            .as_array()
            .unwrap()
            .iter()
            .zip(expected.intended_writes())
        {
            assert_eq!(
                actual["path"],
                serde_json::to_value(NativePath::from_native(&write.path).unwrap()).unwrap()
            );
            assert_eq!(actual["mode"], write.mode);
            assert_eq!(actual["bytes"], write.bytes);
        }
        assert_eq!(
            data["targets"]
                .as_array()
                .unwrap()
                .iter()
                .map(|target| target["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["claude-code", "codex", "opencode"]
        );
        assert_eq!(before, tree(&fixture.0));
    }
}
#[test]
fn existing_identical_and_conflicting_payloads_are_read_only_and_have_distinct_exit_codes() {
    let fixture = Fixture::new();
    let source = fixture.source(OsStr::new("source"));
    let (home, project) = roots(&fixture);
    let plan = library(&source, &home, &project, Scope::Project, &["codex"]);
    assert!(apply_plan(&plan, ApplyOptions::default()).is_successful());
    let command = args(&source, &home, &project, "project", &["codex"]);
    let before = tree(&fixture.0);
    let output = cli(&command);
    assert!(output.status.success());
    assert_eq!(
        json(&output)["data"]["destinations"][0]["action"],
        "unchanged"
    );
    assert_eq!(before, tree(&fixture.0));
    let edit = plan.destinations()[0].path().unwrap().join("resource.ps1");
    fs::write(&edit, b"manual edit").unwrap();
    let before = tree(&fixture.0);
    let output = cli(&command);
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output);
    assert_eq!(value["status"], "blocked");
    assert_eq!(value["data"]["destinations"][0]["action"], "conflict");
    assert_eq!(before, tree(&fixture.0));
    assert_eq!(fs::read(edit).unwrap(), b"manual edit");
}
#[test]
fn unknown_targets_invalid_scopes_missing_roots_and_conflicting_options_never_write() {
    let fixture = Fixture::new();
    let source = fixture.source(OsStr::new("source"));
    let (home, project) = roots(&fixture);
    let cases = vec![
        args(&source, &home, &project, "project", &["unknown"]),
        args(&source, &home, &project, "workspace", &["codex"]),
        vec![
            "plan".into(),
            source.as_os_str().to_owned(),
            "--scope".into(),
            "user".into(),
            "--target".into(),
            "codex".into(),
            "--json".into(),
        ],
    ];
    for command in cases {
        let before = tree(&fixture.0);
        let output = cli(command);
        assert_eq!(output.status.code(), Some(2));
        json(&output);
        assert_eq!(before, tree(&fixture.0));
    }
    for extra in [
        vec!["--scope", "user"],
        vec!["--yes"],
        vec!["--target"],
        vec!["--dry-run"],
    ] {
        let mut command = args(&source, &home, &project, "project", &["codex"]);
        command.extend(extra.into_iter().map(OsString::from));
        let before = tree(&fixture.0);
        let output = cli(command);
        assert_eq!(output.status.code(), Some(2));
        json(&output);
        assert_eq!(before, tree(&fixture.0));
    }
}
#[test]
fn human_plan_explains_cross_host_visibility_and_never_infers_home() {
    use std::process::{Command, Stdio};
    let fixture = Fixture::new();
    let source = fixture.source(OsStr::new("source"));
    let (home, project) = roots(&fixture);
    let mut command = args(
        &source,
        &home,
        &project,
        "user",
        &["opencode", "claude-code"],
    );
    command.retain(|value| value != "--json");
    let ignored = fixture.0.join("ignored ambient home");
    let output = Command::new(env!("CARGO_BIN_EXE_agent-skills-kit"))
        .args(command)
        .env("HOME", &ignored)
        .env("USERPROFILE", &ignored)
        .env("CLAUDE_CONFIG_DIR", &ignored)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("compatible hosts may discover"));
    assert!(text.contains("Real host discovery has not been tested"));
    assert!(!ignored.exists());
}

#[test]
fn explicit_parent_components_are_not_canonicalized_into_authority() {
    let fixture = Fixture::new();
    let source = fixture.source(OsStr::new("source"));
    let (home, project) = roots(&fixture);
    let traversal = home.join("..").join("home with spaces");
    let before = tree(&fixture.0);
    let output = cli(args(&source, &traversal, &project, "user", &["codex"]));
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output);
    assert_eq!(value["status"], "blocked");
    assert!(
        value["data"]["destinations"][0]["problems"]
            .to_string()
            .contains("ParentComponent")
    );
    assert_eq!(before, tree(&fixture.0));
}
