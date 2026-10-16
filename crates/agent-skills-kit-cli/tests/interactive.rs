use std::process::{Command, Stdio};
#[test]
fn redirected_input_refuses_interactive_mode_without_reading_or_printing_prompts() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-skills-kit"))
        .args(["interactive", "--json"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    assert!(!output.stdout.contains(&0x1b));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["status"], "error");
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("terminal stdin and stderr")
    );
}

#[path = "common/fixture.rs"]
mod fixture;
#[test]
fn piped_answers_cannot_bypass_the_terminal_requirement() {
    use std::{ffi::OsStr, fs, io::Write};
    let fixture = fixture::Fixture::new();
    let source = fixture.source(OsStr::new("source"));
    let root = fixture.0.join("project");
    fs::create_dir(&root).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_agent-skills-kit"))
        .args(["interactive", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let sent = input.write_all(
        format!(
            "{}\ncodex\nproject\n{}\nyes\n",
            source.display(),
            root.display()
        )
        .as_bytes(),
    );
    drop(input);
    let output = child.wait_with_output().unwrap();
    if let Err(error) = sent {
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::BrokenPipe,
            "early refusal may close stdin"
        );
    }
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "error");
    assert_eq!(fs::read_dir(root).unwrap().count(), 0);
}
