use agent_skills_kit::catalog::Scope;
use std::{ffi::OsString, path::PathBuf};

#[derive(Debug)]
pub(crate) enum Command {
    Help,
    Version,
    Targets,
    Target(String),
    Validate(PathBuf),
    Plan(PlanArgs),
}
pub(crate) fn json_requested(args: &[OsString]) -> bool {
    args.iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--json")
}
#[derive(Debug)]
pub(crate) struct PlanArgs {
    pub source: PathBuf,
    pub targets: Vec<String>,
    pub scope: Scope,
    pub user_home: Option<PathBuf>,
    pub project_root: Option<PathBuf>,
}
pub(crate) fn parse(args: &[OsString]) -> Result<Command, String> {
    let mut positional = vec![];
    let mut literal = false;
    let mut json = false;
    let mut targets = vec![];
    let mut scope = None;
    let mut user_home = None;
    let mut project_root = None;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        index += 1;
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
        } else if ["--target", "--scope", "--user-home", "--project-root"]
            .iter()
            .any(|name| arg == name)
        {
            let value = args
                .get(index)
                .ok_or_else(|| format!("{} requires a value", arg.to_string_lossy()))?;
            if value.to_str().is_some_and(|value| value.starts_with('-')) {
                return Err(format!(
                    "{} requires a value; prefix dash-leading paths with ./",
                    arg.to_string_lossy()
                ));
            }
            index += 1;
            if arg == "--target" {
                targets.push(value.to_str().ok_or("target ID must be UTF-8")?.to_owned());
            } else if arg == "--scope" {
                if scope.is_some() {
                    return Err("--scope may appear only once".into());
                }
                scope = Some(match value.to_str() {
                    Some("user") => Scope::User,
                    Some("project") => Scope::Project,
                    _ => return Err("--scope must be user or project".into()),
                });
            } else {
                let slot = if arg == "--user-home" {
                    &mut user_home
                } else {
                    &mut project_root
                };
                if slot.is_some() {
                    return Err(format!("{} may appear only once", arg.to_string_lossy()));
                }
                *slot = Some(PathBuf::from(value));
            }
        } else if arg.to_str().is_some_and(|arg| arg.starts_with('-')) {
            return Err(format!("unknown option: {}", arg.to_string_lossy()));
        } else {
            positional.push(arg);
        }
    }
    if let [command, source] = positional.as_slice() {
        if command.as_os_str() == "plan" {
            let scope = scope.ok_or("plan requires --scope user or project")?;
            if targets.is_empty() {
                return Err("plan requires at least one --target".into());
            }
            if (scope == Scope::User && user_home.is_none())
                || (scope == Scope::Project && project_root.is_none())
            {
                return Err(
                    "supply --user-home for user scope or --project-root for project scope".into(),
                );
            }
            return Ok(Command::Plan(PlanArgs {
                source: PathBuf::from(source),
                targets,
                scope,
                user_home,
                project_root,
            }));
        }
    }
    if !targets.is_empty() || scope.is_some() || user_home.is_some() || project_root.is_some() {
        return Err("target/scope/context options require the plan command".into());
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
        _ => Err(
            "expected targets list/show, validate <source>, or plan <source>; see --help".into(),
        ),
    }
}
