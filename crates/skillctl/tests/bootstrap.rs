use std::process::Command;

#[test]
fn executable_reports_bootstrap_status() {
    let output = Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .output()
        .expect("bootstrap executable should start");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let expected = include_str!("fixtures/bootstrap.stdout");
    let stdout = String::from_utf8(output.stdout).expect("status should be UTF-8");
    assert_eq!(stdout.replace("\r\n", "\n"), expected);
    assert_eq!(stdout.trim_end(), agent_skills_kit::STATUS);
}
