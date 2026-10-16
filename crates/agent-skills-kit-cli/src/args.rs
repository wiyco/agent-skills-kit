use std::{ffi::OsString, path::PathBuf};

#[derive(Debug)]
pub(crate) enum Command {
    Help,
    Version,
    Targets,
    Target(String),
    Validate(PathBuf),
}
pub(crate) fn json_requested(args: &[OsString]) -> bool {
    args.iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--json")
}
pub(crate) fn parse(args: &[OsString]) -> Result<Command, String> {
    let mut positional = vec![];
    let mut literal = false;
    let mut json = false;
    for arg in args {
        if literal {
            positional.push(arg);
        } else if arg == "--" {
            literal = true;
        } else if arg == "--json" {
            if json {
                return Err("--json may appear only once".into());
            }
            json = true;
        } else if arg == "--help" || arg == "-h" {
            return Ok(Command::Help);
        } else if arg == "--version" || arg == "-V" {
            if args.len() != 1 {
                return Err("--version must be used alone".into());
            }
            return Ok(Command::Version);
        } else if arg.to_str().is_some_and(|arg| arg.starts_with('-')) {
            return Err(format!("unknown option: {}", arg.to_string_lossy()));
        } else {
            positional.push(arg);
        }
    }
    match positional.as_slice() {
        [] => Ok(Command::Help),
        [group, sub] if group.as_os_str() == "targets" && sub.as_os_str() == "list" => {
            Ok(Command::Targets)
        }
        [group, sub, target] if group.as_os_str() == "targets" && sub.as_os_str() == "show" => Ok(
            Command::Target(target.to_str().ok_or("target ID must be UTF-8")?.to_owned()),
        ),
        [command, source] if command.as_os_str() == "validate" => {
            Ok(Command::Validate(PathBuf::from(source)))
        }
        _ => {
            Err("expected targets list, targets show <id>, or validate <source>; see --help".into())
        }
    }
}
