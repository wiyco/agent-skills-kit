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
