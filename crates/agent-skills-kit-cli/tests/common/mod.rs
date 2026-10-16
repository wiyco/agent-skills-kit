mod fixture;
pub use fixture::Fixture;
use serde_json::Value;
use std::{
    ffi::OsStr,
    process::{Command, Output, Stdio},
};
pub fn cli(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_agent-skills-kit"))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}
pub fn json(output: &Output) -> Value {
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    assert!(!output.stdout.contains(&0x1b));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    value
}
