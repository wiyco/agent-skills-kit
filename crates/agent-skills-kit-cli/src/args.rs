use agent_skills_kit::catalog::Scope;
use std::{ffi::OsString, path::PathBuf};

#[derive(Debug)]
pub(crate) enum Command {
    Help,
    Interactive,
    Manage,
    Skills {
        selection: SelectionArgs,
        action: SkillAction,
    },
    Version,
    Targets,
    Target(String),
    Validate(PathBuf),
    Plan(PlanArgs),
    Install {
        plan: PlanArgs,
        yes: bool,
        dry_run: bool,
    },
}
#[derive(Debug)]
pub(crate) enum SkillAction {
    List,
    Copy {
        name: OsString,
        targets: Vec<String>,
        yes: bool,
        dry_run: bool,
    },
    Delete {
        name: OsString,
        confirmation: Option<String>,
        dry_run: bool,
    },
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
#[derive(Debug)]
pub(crate) struct SelectionArgs {
    pub agent: String,
    pub scope: Scope,
    pub user_home: Option<PathBuf>,
    pub project_root: Option<PathBuf>,
    pub location: Option<usize>,
}
pub(crate) fn parse(args: &[OsString]) -> Result<Command, String> {
    let mut positional = vec![];
    let mut literal = false;
    let mut json = false;
    let mut targets = vec![];
    let mut scope = None;
    let mut user_home = None;
    let mut project_root = None;
    let mut yes = false;
    let mut dry_run = false;
    let mut from = None;
    let mut location = None;
    let mut confirmation = None;
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
        } else if arg == "--yes" || arg == "--dry-run" {
            let flag = if arg == "--yes" {
                &mut yes
            } else {
                &mut dry_run
            };
            if *flag {
                return Err(format!("{} may appear only once", arg.to_string_lossy()));
            }
            *flag = true;
        } else if arg == "--help" || arg == "-h" {
            return Ok(Command::Help);
        } else if arg == "--version" || arg == "-V" {
            if args.len() != 1 {
                return Err("--version must be used alone".into());
            }
            return Ok(Command::Version);
        } else if [
            "--target",
            "--scope",
            "--user-home",
            "--project-root",
            "--from",
            "--location",
            "--confirm-delete",
        ]
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
            if arg == "--confirm-delete" {
                if confirmation.is_some() {
                    return Err("--confirm-delete may appear only once".into());
                }
                confirmation = Some(
                    value
                        .to_str()
                        .ok_or("confirmation must be UTF-8")?
                        .to_owned(),
                );
            } else if arg == "--from" {
                if from.is_some() {
                    return Err("--from may appear only once".into());
                }
                from = Some(value.to_str().ok_or("agent ID must be UTF-8")?.to_owned());
            } else if arg == "--location" {
                if location.is_some() {
                    return Err("--location may appear only once".into());
                }
                let number = value
                    .to_str()
                    .and_then(|x| x.parse::<usize>().ok())
                    .filter(|x| *x > 0)
                    .ok_or("--location must be a positive displayed location number")?;
                location = Some(number);
            } else if arg == "--target" {
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
    if positional
        .first()
        .is_some_and(|x| x.as_os_str() == "skills")
    {
        let is_list = positional.len() == 2 && positional[1].as_os_str() == "list";
        let is_copy = positional.len() == 3 && positional[1].as_os_str() == "copy";
        let is_delete = positional.len() == 3 && positional[1].as_os_str() == "delete";
        if !is_list && !is_copy && !is_delete {
            return Err("expected skills list, skills copy <name>, or skills delete <name>".into());
        }
        let scope = scope.ok_or("skills requires --scope user or project")?;
        if (scope == Scope::User && user_home.is_none())
            || (scope == Scope::Project && project_root.is_none())
        {
            return Err("supply the explicit root for the selected scope".into());
        }
        if is_list && (!targets.is_empty() || yes || dry_run || location.is_some()) {
            return Err("copy options cannot be used with skills list".into());
        }
        if is_copy && (targets.is_empty() || location.is_none()) {
            return Err("skills copy requires --location and at least one --target".into());
        }
        if !is_delete && confirmation.is_some() {
            return Err("--confirm-delete requires skills delete".into());
        }
        if is_delete && (yes || !targets.is_empty() || location.is_none()) {
            return Err("skills delete requires --location and forbids --yes/--target".into());
        }
        if is_delete && dry_run && confirmation.is_some() {
            return Err("--dry-run cannot be combined with --confirm-delete".into());
        }
        return Ok(Command::Skills {
            selection: SelectionArgs {
                agent: from.ok_or("skills requires --from <agent>")?,
                scope,
                user_home,
                project_root,
                location,
            },
            action: if is_copy {
                SkillAction::Copy {
                    name: positional[2].to_owned(),
                    targets,
                    yes,
                    dry_run,
                }
            } else if is_delete {
                SkillAction::Delete {
                    name: positional[2].to_owned(),
                    confirmation,
                    dry_run,
                }
            } else {
                SkillAction::List
            },
        });
    }
    if confirmation.is_some() {
        return Err("--confirm-delete requires skills delete".into());
    }
    if from.is_some() || location.is_some() {
        return Err("--from and --location require skills list/copy".into());
    }
    if let [command, source] = positional.as_slice() {
        if command.as_os_str() == "plan" || command.as_os_str() == "install" {
            let scope = scope.ok_or("plan/install requires --scope user or project")?;
            if targets.is_empty() {
                return Err("plan/install requires at least one --target".into());
            }
            if (scope == Scope::User && user_home.is_none())
                || (scope == Scope::Project && project_root.is_none())
            {
                return Err(
                    "supply --user-home for user scope or --project-root for project scope".into(),
                );
            }
            if command.as_os_str() == "plan" && (yes || dry_run) {
                return Err("--yes and --dry-run require install".into());
            }
            let plan = PlanArgs {
                source: PathBuf::from(source),
                targets,
                scope,
                user_home,
                project_root,
            };
            return Ok(if command.as_os_str() == "install" {
                Command::Install { plan, yes, dry_run }
            } else {
                Command::Plan(plan)
            });
        }
    }
    if yes || dry_run {
        return Err("--yes and --dry-run require install".into());
    }
    if !targets.is_empty() || scope.is_some() || user_home.is_some() || project_root.is_some() {
        return Err("target/scope/context options require plan or install".into());
    }
    match positional.as_slice() {
        [] => Ok(Command::Help),
        [command] if command.as_os_str() == "interactive" => Ok(Command::Interactive),
        [command] if command.as_os_str() == "manage" => Ok(Command::Manage),
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
            "expected targets list/show, validate <source>, plan <source>, or install <source>; see --help".into(),
        ),
    }
}
