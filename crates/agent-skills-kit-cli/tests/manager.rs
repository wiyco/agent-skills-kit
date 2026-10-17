mod common;
use common::{Fixture, cli, json};
use std::{
    ffi::{OsStr, OsString},
    fs,
};

fn setup() -> (Fixture, std::path::PathBuf) {
    let fixture = Fixture::new();
    let home = fs::canonicalize(&fixture.0).unwrap().join("home");
    fs::create_dir(&home).unwrap();
    let seed = fixture.source(OsStr::new("seed"));
    let source = home.join(".agents/skills/sample");
    fs::create_dir_all(&source).unwrap();
    fs::copy(seed.join("SKILL.md"), source.join("SKILL.md")).unwrap();
    fs::copy(seed.join("resource.ps1"), source.join("resource.ps1")).unwrap();
    (fixture, home)
}
fn arguments(home: &std::path::Path, copy: bool) -> Vec<OsString> {
    let mut args = vec!["skills", if copy { "copy" } else { "list" }]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    if copy {
        args.extend(["sample", "--location", "1", "--target", "claude-code"].map(OsString::from));
    }
    args.extend(["--from", "codex-cli", "--scope", "user", "--user-home"].map(OsString::from));
    args.push(home.into());
    args.push("--json".into());
    args
}
#[test]
fn scripted_list_copy_approval_dry_run_unchanged_and_conflict_use_native_engine() {
    let (_fixture, home) = setup();
    let listed = cli(arguments(&home, false));
    assert!(listed.status.success(), "{listed:?}");
    let value = json(&listed);
    assert_eq!(value["command"], "skills.list");
    assert_eq!(value["data"]["target_id"], "codex");
    assert_eq!(
        value["data"]["locations"][0]["skills"][0]["selectable"],
        true
    );
    let mut args = arguments(&home, true);
    let output = cli(&args);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(json(&output)["data"]["apply_attempted"], false);
    assert!(!home.join(".claude").exists());
    args.push("--dry-run".into());
    assert!(cli(&args).status.success());
    assert!(!home.join(".claude").exists());
    args.pop();
    args.push("--yes".into());
    let output = cli(&args);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(json(&output)["command"], "skills.copy");
    let resource = home.join(".claude/skills/sample/resource.ps1");
    assert_eq!(
        fs::read(&resource).unwrap(),
        b"inert resource; never executed"
    );
    let output = cli(&args);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        json(&output)["data"]["report"]["outcomes"][0]["status"],
        "unchanged"
    );
    fs::write(&resource, b"existing different data").unwrap();
    let output = cli(&args);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json(&output)["data"]["apply_attempted"], false);
    assert_eq!(fs::read(resource).unwrap(), b"existing different data");
}
#[test]
fn same_location_is_explicitly_unchanged_and_invalid_rows_cannot_be_selected() {
    let (_fixture, home) = setup();
    let mut args = arguments(&home, true);
    let index = args.iter().position(|x| x == "claude-code").unwrap();
    args[index] = "codex".into();
    args.push("--yes".into());
    let output = cli(&args);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        json(&output)["data"]["report"]["outcomes"][0]["status"],
        "unchanged"
    );
    fs::write(
        home.join(".agents/skills/sample/SKILL.md"),
        b"invalid metadata",
    )
    .unwrap();
    let output = cli(arguments(&home, false));
    assert!(output.status.success());
    assert_eq!(
        json(&output)["data"]["locations"][0]["skills"][0]["selectable"],
        false
    );
    let output = cli(&args);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        fs::read(home.join(".agents/skills/sample/SKILL.md")).unwrap(),
        b"invalid metadata"
    );
}
#[test]
fn management_refuses_non_terminal_and_invalid_options_without_writes() {
    let (_fixture, home) = setup();
    let output = cli(["manage", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        json(&output)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("terminal")
    );
    for tail in [
        vec!["--yes"],
        vec!["--location", "1"],
        vec!["--target", "claude-code"],
        vec!["--from", "claude-code"],
    ] {
        let mut args = arguments(&home, false);
        args.extend(tail.into_iter().map(OsString::from));
        let output = cli(args);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
    }
    assert!(!home.join(".claude").exists());
}

#[cfg(unix)]
#[test]
fn unreadable_selected_agent_root_is_reported_and_cannot_authorize_copy() {
    use std::os::unix::fs::PermissionsExt;
    let (_fixture, home) = setup();
    let root = home.join(".agents/skills");
    struct Restore(std::path::PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
    let guard = Restore(root.clone());
    fs::set_permissions(&root, fs::Permissions::from_mode(0o000)).unwrap();
    let output = cli(arguments(&home, false));
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json(&output)["data"]["complete"], false);
    assert_eq!(json(&output)["data"]["locations"][0]["missing"], false);
    let mut args = arguments(&home, true);
    args.push("--yes".into());
    let output = cli(args);
    assert!(!output.status.success());
    assert!(!home.join(".claude").exists());
    drop(guard);
}

#[cfg(windows)]
#[test]
fn sharing_denied_skill_remains_an_explicit_unselectable_cli_row() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_fixture, home) = setup();
    let guard = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(home.join(".agents/skills/sample/SKILL.md"))
        .unwrap();
    let output = cli(arguments(&home, false));
    assert!(output.status.success());
    assert_eq!(
        json(&output)["data"]["locations"][0]["skills"][0]["selectable"],
        false
    );
    let mut args = arguments(&home, true);
    args.push("--yes".into());
    assert!(!cli(args).status.success());
    assert!(!home.join(".claude").exists());
    drop(guard);
}
