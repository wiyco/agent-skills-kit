mod common;
use agent_skills_kit::{catalog::Catalog, receipt::NativePath};
use common::{Fixture, cli, json};
use std::{
    ffi::{OsStr, OsString},
    fs,
    process::{Command, Stdio},
};
#[test]
fn help_version_and_empty_input_never_read_redirected_stdin() {
    for args in [vec![], vec!["--help"], vec!["-h"]] {
        let output = cli(args);
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("targets list"));
        assert!(text.contains("not exclusive visibility"));
    }
    let version = cli(["--version"]);
    assert!(version.status.success());
    assert!(version.stderr.is_empty());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        format!("agent-skills-kit {}", env!("CARGO_PKG_VERSION"))
    );
}
#[test]
fn catalog_json_matches_library_and_aliases_keep_canonical_identity() {
    let output = cli(["targets", "list", "--json"]);
    assert!(output.status.success());
    let value = json(&output);
    assert_eq!(value["command"], "targets.list");
    assert_eq!(
        value["data"],
        serde_json::to_value(Catalog::bundled().unwrap().document()).unwrap()
    );
    let output = cli(["--json", "targets", "show", "codex-cli"]);
    assert!(output.status.success());
    assert_eq!(json(&output)["data"]["id"], "codex");
}
#[test]
fn validation_is_read_only_and_preserves_original_bytes() {
    let fixture = Fixture::new();
    let source = fixture.source(OsStr::new("space café"));
    let before = fs::read(source.join("SKILL.md")).unwrap();
    let output = cli([
        OsStr::new("validate"),
        source.as_os_str(),
        OsStr::new("--json"),
    ]);
    assert!(output.status.success(), "{:?}", output.stderr);
    let value = json(&output);
    assert_eq!(value["data"]["name"], "sample");
    assert_eq!(
        value["data"]["source"],
        serde_json::to_value(NativePath::from_native(&source).unwrap()).unwrap()
    );
    assert_eq!(fs::read(source.join("SKILL.md")).unwrap(), before);
    assert_eq!(
        fs::read(source.join("resource.ps1")).unwrap(),
        b"inert resource; never executed"
    );
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
    assert_eq!(fs::read_dir(&source).unwrap().count(), 3);
}
#[test]
fn invalid_commands_options_and_targets_return_usage_errors_without_stdout_noise() {
    for args in [
        vec!["targets"],
        vec!["targets", "show", "unknown"],
        vec!["validate"],
        vec!["--yes"],
        vec!["--json", "--json"],
        vec!["--version", "--json"],
    ] {
        let json_mode = args.contains(&"--json");
        let output = cli(args);
        assert_eq!(output.status.code(), Some(2));
        if json_mode {
            assert_eq!(json(&output)["status"], "error");
        } else {
            assert!(output.stdout.is_empty());
            assert!(
                String::from_utf8(output.stderr)
                    .unwrap()
                    .starts_with("error: ")
            );
        }
    }
}
#[test]
fn invalid_skill_is_an_operational_error_and_never_executes_resources() {
    let fixture = Fixture::new();
    let source = fixture.source(OsStr::new("invalid"));
    fs::write(
        source.join("SKILL.md"),
        b"---\nname: sample\n---\nMissing description\n",
    )
    .unwrap();
    let output = cli([
        OsStr::new("validate"),
        source.as_os_str(),
        OsStr::new("--json"),
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(json(&output)["error"]["exit_code"], 1);
    assert_eq!(fs::read_dir(&source).unwrap().count(), 3);
}
#[test]
fn untrusted_diagnostics_are_escaped_in_human_and_json_output() {
    let target = "bad\u{1b}[31m\u{009b}31m\u{202e}\nforged";
    let output = cli(["targets", "show", target]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let text = String::from_utf8(output.stderr).unwrap();
    assert!(!text.contains(['\u{1b}', '\u{009b}', '\u{202e}']));
    assert_eq!(text.lines().count(), 1);
    let output = cli(["targets", "show", target, "--json"]);
    let value = json(&output);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains(['\u{009b}', '\u{202e}']));
    assert_eq!(
        value["error"]["message"],
        format!("unknown target: {target}")
    );
}
#[test]
fn native_non_unicode_paths_round_trip_in_json() {
    let fixture = Fixture::new();
    #[cfg(unix)]
    let parent = {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(b"opaque-\xff".to_vec())
    };
    #[cfg(windows)]
    let parent = {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[b'o' as u16, 0xd800])
    };
    let source_parent = fixture.0.join(&parent);
    match fs::create_dir(&source_parent) {
        Ok(()) => (),
        #[cfg(target_os = "macos")]
        Err(error) => {
            assert_eq!(
                error.raw_os_error(),
                Some(92),
                "APFS native-name refusal must be explicit"
            );
            return;
        }
        #[cfg(not(target_os = "macos"))]
        Err(error) => panic!("required native-name fixture: {error}"),
    }
    let source = fixture.source(&parent);
    let output = cli([
        OsStr::new("validate"),
        source.as_os_str(),
        OsStr::new("--json"),
    ]);
    assert!(output.status.success(), "{:?}", output.stderr);
    assert_eq!(
        json(&output)["data"]["source"],
        serde_json::to_value(NativePath::from_native(&source).unwrap()).unwrap()
    );
}

#[test]
fn relative_source_uses_only_the_childs_explicit_working_directory() {
    let fixture = Fixture::new();
    let source = fixture.source(OsStr::new("relative"));
    let output = Command::new(env!("CARGO_BIN_EXE_agent-skills-kit"))
        .current_dir(source.parent().unwrap())
        .args(["validate", "sample", "--json"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    let reported: NativePath =
        serde_json::from_value(json(&output)["data"]["source"].clone()).unwrap();
    let native = reported.to_native().unwrap();
    assert!(native.is_absolute());
    // current_dir may expose a physical ancestor spelling (macOS /private/var).
    assert_eq!(
        fs::canonicalize(native).unwrap(),
        fs::canonicalize(source).unwrap()
    );
}
