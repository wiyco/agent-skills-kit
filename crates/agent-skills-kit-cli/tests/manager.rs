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

fn delete_arguments(home: &std::path::Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "skills",
        "delete",
        "sample",
        "--location",
        "1",
        "--from",
        "codex",
        "--scope",
        "user",
        "--user-home",
    ]
    .map(OsString::from)
    .into();
    args.push(home.into());
    args.push("--json".into());
    args
}
#[test]
fn permanent_delete_requires_exact_current_fingerprint_and_preserves_other_copies() {
    let (_fixture, home) = setup();
    let mut copy = arguments(&home, true);
    copy.push("--yes".into());
    assert!(cli(copy).status.success());
    let args = delete_arguments(&home);
    let preview = cli(&args);
    assert_eq!(preview.status.code(), Some(3), "{preview:?}");
    let data = json(&preview)["data"].clone();
    assert_eq!(data["apply_attempted"], false);
    assert_eq!(data["entries"].as_array().unwrap().len(), 2);
    assert!(
        data["known_readers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["target_id"] == "codex")
    );
    assert!(!data["unresolved_readers"].as_array().unwrap().is_empty());
    let token = data["confirmation_token"].as_str().unwrap();
    let mut dry = args.clone();
    dry.push("--dry-run".into());
    assert!(cli(dry).status.success());
    for extra in [
        vec!["--yes"],
        vec!["--target", "claude-code"],
        vec!["--confirm-delete", "wrong"],
        vec!["--dry-run", "--confirm-delete", token],
    ] {
        let mut refused = args.clone();
        refused.extend(extra.into_iter().map(OsString::from));
        assert!(!cli(refused).status.success());
        assert!(home.join(".agents/skills/sample/SKILL.md").is_file());
    }
    let mut approved = args.clone();
    approved.extend(["--confirm-delete", token].map(OsString::from));
    let output = cli(approved);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(json(&output)["data"]["report"]["successful"], true);
    assert!(!home.join(".agents/skills/sample").exists());
    assert_eq!(
        fs::read(home.join(".claude/skills/sample/resource.ps1")).unwrap(),
        b"inert resource; never executed"
    );
}
#[test]
fn changed_payload_or_reader_impact_invalidates_previous_delete_confirmation() {
    for change_readers in [false, true] {
        let (_fixture, home) = setup();
        let args = delete_arguments(&home);
        let preview = json(&cli(&args));
        let token = preview["data"]["confirmation_token"].as_str().unwrap();
        if change_readers {
            // An uninspectable configured container changes unresolved-reader evidence.
            fs::create_dir_all(home.join(".claude")).unwrap();
            fs::write(home.join(".claude/skills"), b"not a directory").unwrap();
        } else {
            fs::write(
                home.join(".agents/skills/sample/resource.ps1"),
                b"edited after preview",
            )
            .unwrap();
        }
        let mut approved = args;
        approved.extend(["--confirm-delete", token].map(OsString::from));
        let output = cli(approved);
        assert!(!output.status.success(), "{output:?}");
        assert!(home.join(".agents/skills/sample/SKILL.md").is_file());
        assert!(
            fs::read_dir(home.join(".agents/skills"))
                .unwrap()
                .all(|e| e.unwrap().file_name() == "sample")
        );
    }
}

#[test]
fn actual_native_partial_delete_keeps_complete_cli_outcomes_and_retained_diagnostics() {
    let (_fixture, home) = setup();
    let source = home.join(".agents/skills/sample");
    fs::create_dir(source.join("aaa")).unwrap();
    fs::write(source.join("aaa/first"), b"approved first removal").unwrap();
    fs::create_dir(source.join("zzz")).unwrap();
    fs::write(
        source.join("zzz/refused"),
        b"must remain after permission refusal",
    )
    .unwrap();
    #[cfg(unix)]
    let restricted = source.join("zzz");
    #[cfg(windows)]
    let restricted = source.join("zzz/refused");
    let mut permissions = fs::metadata(&restricted).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&restricted, permissions).unwrap();
    let args = delete_arguments(&home);
    let preview = json(&cli(&args));
    let token = preview["data"]["confirmation_token"].as_str().unwrap();
    let mut approved = args;
    approved.extend(["--confirm-delete", token].map(OsString::from));
    let output = cli(approved);
    let result = json(&output);
    // Restore only the fixture-owned entry for teardown, after recording the
    // actual native result. A production diagnostic path grants no authority.
    let stage = fs::read_dir(home.join(".agents/skills"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.file_name().unwrap() != "sample")
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = stage.join("payload/zzz");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o222, 0);
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(windows)]
    {
        let path = stage.join("payload/zzz/refused");
        let mut permission = fs::metadata(&path).unwrap().permissions();
        assert!(permission.readonly());
        permission.set_readonly(false);
        fs::set_permissions(path, permission).unwrap();
    }
    assert_eq!(output.status.code(), Some(4), "{output:?}");
    let report = &result["data"]["report"];
    assert_eq!(report["successful"], false);
    assert_eq!(report["permanent_removal_attempted"], true);
    assert!(report["retained_stage"].is_object());
    let outcomes = report["outcomes"].as_array().unwrap();
    assert_eq!(
        outcomes.len(),
        preview["data"]["entries"].as_array().unwrap().len() + 1
    );
    for state in ["removed", "remaining", "not_attempted"] {
        assert!(outcomes.iter().any(|r| r["state"] == state), "{report}");
    }
    assert!(report["failure"].is_object());
    assert_eq!(
        fs::read(stage.join("payload/zzz/refused")).unwrap(),
        b"must remain after permission refusal"
    );
}
