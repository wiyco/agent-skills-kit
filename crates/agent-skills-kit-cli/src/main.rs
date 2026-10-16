//! Thin terminal consumer of the reusable library. Skill data is never executed.
mod args;
mod installation;
mod planning;
use agent_skills_kit::{
    catalog::Catalog,
    receipt::NativePath,
    validation::{InventoryLimits, inventory_skill},
};
use args::Command;
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    io::{self, Write},
    process::ExitCode,
};

const HELP: &str = "Agent Skills Kit (Rust)

Usage:
  agent-skills-kit targets list [--json]
  agent-skills-kit targets show <id> [--json]
  agent-skills-kit validate <source> [--json]
  agent-skills-kit plan <source> --target <id>... --scope <user|project> [context] [--json]
  agent-skills-kit install <source> --target <id>... --scope <user|project> [context] [--yes] [--dry-run] [--json]
  agent-skills-kit --help
  agent-skills-kit --version

Commands never execute skill content. Non-interactive install never prompts.
Use -- before a positional path beginning with a dash.
Selecting a host means placement, not exclusive visibility to that host.
Context: --user-home <path> for user scope; --project-root <path> for project scope.
No HOME or host overrides are inferred. Install requires --yes; --dry-run never writes.
";
struct Output {
    exit_code: u8,
    command: &'static str,
    data: Value,
    human: String,
}
struct Failure {
    code: u8,
    message: String,
}
impl Failure {
    fn operation(error: impl std::fmt::Display) -> Self {
        Self {
            code: 1,
            message: error.to_string(),
        }
    }
    fn usage(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: message.into(),
        }
    }
}
// Human output must not interpret untrusted controls or directional overrides.
// JSON uses serde_json escaping and lossless tagged native paths separately.
fn terminal_text(text: &str) -> String {
    text.chars().flat_map(|character| {
        if (character.is_control() && character != '\n' && character != '\t')
            || matches!(character, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
            character.escape_default().collect::<Vec<_>>()
        } else { vec![character] }
    }).collect()
}
fn json_line(value: Value) -> String {
    // JSON remains valid and round-trips the original characters while ensuring
    // terminals cannot interpret C1 controls or directional formatting bytes.
    value.to_string().chars().map(|character| {
        if matches!(character, '\u{0080}'..='\u{009f}' | '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
            format!("\\u{:04x}", character as u32)
        } else { character.to_string() }
    }).collect()
}
fn absolute(path: std::path::PathBuf) -> Result<std::path::PathBuf, Failure> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()
            .map_err(Failure::operation)?
            .join(path))
    }
}
fn execute(command: Command) -> Result<Output, Failure> {
    match command {
        Command::Plan(options) => planning::execute(options),
        Command::Install { plan, yes, dry_run } => installation::execute(plan, yes, dry_run),
        Command::Help => Ok(Output {
            exit_code: 0,
            command: "help",
            data: json!({"usage": HELP}),
            human: HELP.into(),
        }),
        Command::Version => Ok(Output {
            exit_code: 0,
            command: "version",
            data: json!({"version": env!("CARGO_PKG_VERSION")}),
            human: format!("agent-skills-kit {}\n", env!("CARGO_PKG_VERSION")),
        }),
        Command::Targets => {
            let catalog = Catalog::bundled().map_err(Failure::operation)?;
            let mut human = catalog
                .targets()
                .map(|target| format!("{} ({}/{})\n", target.id, target.product_id, target.variant))
                .collect::<String>();
            human.push_str("Placement does not imply exclusive visibility; inspect target discovery locations.\n");
            Ok(Output {
                exit_code: 0,
                command: "targets.list",
                data: json!(catalog.document()),
                human,
            })
        }
        Command::Target(id) => {
            let catalog = Catalog::bundled().map_err(Failure::operation)?;
            let target = catalog
                .get(&id)
                .ok_or_else(|| Failure::usage(format!("unknown target: {id}")))?;
            let data = json!(target);
            let human = format!(
                "{}\nPlacement does not imply exclusive visibility; discovery locations are listed above.\n",
                serde_json::to_string_pretty(&data).map_err(Failure::operation)?
            );
            Ok(Output {
                exit_code: 0,
                command: "targets.show",
                data,
                human,
            })
        }
        Command::Validate(path) => {
            let path = absolute(path)?;
            let skill =
                inventory_skill(&path, InventoryLimits::default()).map_err(Failure::operation)?;
            let data = json!({
                "source": NativePath::from_native(&path).map_err(Failure::operation)?,
                "name": skill.metadata().name(),
                "description": skill.metadata().description(),
                "resource_entries": skill.inventory().entries().len(),
                "total_bytes": skill.inventory().total_bytes(),
            });
            let human = format!(
                "Valid inert skill: {}\n{} entries, {} bytes inspected. Validation is not malware review or host discovery.\n",
                skill.metadata().name(),
                skill.inventory().entries().len(),
                skill.inventory().total_bytes()
            );
            Ok(Output {
                exit_code: 0,
                command: "validate",
                data,
                human,
            })
        }
    }
}
fn run(args: &[OsString], stdout: &mut impl Write, stderr: &mut impl Write) -> io::Result<u8> {
    let json = args::json_requested(args);
    let result = args::parse(args).map_err(Failure::usage).and_then(execute);
    match result {
        Ok(output) => {
            if json {
                writeln!(
                    stdout,
                    "{}",
                    json_line(
                        json!({"schema_version": 1, "command": output.command, "status": match output.exit_code { 0 => "ok", 3 => "approval_required", 4 => "failed", _ => "blocked" }, "data": output.data})
                    )
                )?;
            } else {
                write!(stdout, "{}", terminal_text(&output.human))?;
            }
            Ok(output.exit_code)
        }
        Err(error) => {
            if json {
                writeln!(
                    stdout,
                    "{}",
                    json_line(
                        json!({"schema_version": 1, "command": "error", "status": "error", "error": {"exit_code": error.code, "message": error.message}})
                    )
                )?;
            } else {
                writeln!(
                    stderr,
                    "error: {}",
                    terminal_text(&error.message)
                        .replace('\n', "\\n")
                        .replace('\t', "\\t")
                )?;
            }
            Ok(error.code)
        }
    }
}
fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let code = run(&args, &mut io::stdout().lock(), &mut io::stderr().lock()).unwrap_or(1);
    ExitCode::from(code)
}
