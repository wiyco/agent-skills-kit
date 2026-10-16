use crate::{Failure, Output, args::PlanArgs, installation, planning, terminal_text};
use agent_skills_kit::{
    catalog::{Catalog, Scope},
    planner::InstallPlan,
};
use serde_json::json;
use std::{
    io::{BufRead, Read, Write},
    path::PathBuf,
};

const MAX_INPUT: usize = 32768;
fn write(prompts: &mut impl Write, text: &str) -> Result<(), Failure> {
    prompts
        .write_all(terminal_text(text).as_bytes())
        .map_err(Failure::operation)?;
    prompts.flush().map_err(Failure::operation)
}
fn prompt(
    input: &mut impl BufRead,
    prompts: &mut impl Write,
    message: &str,
) -> Result<Option<String>, Failure> {
    write(prompts, message)?;
    let mut bytes = vec![];
    let count = Read::take(&mut *input, MAX_INPUT as u64 + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(Failure::operation)?;
    if count > MAX_INPUT {
        return Err(Failure::usage("prompt input exceeds the 32768-byte budget"));
    }
    // EOF after partial text is cancellation too, especially an unterminated
    // "yes". Only a delimiter-terminated answer can authorize the retained plan.
    if count == 0 || bytes.last() != Some(&b'\n') {
        return Ok(None);
    }
    bytes.pop();
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    let text = String::from_utf8(bytes).map_err(|_| Failure::usage("interactive input must be UTF-8; use non-interactive native arguments for opaque paths"))?;
    if text.trim().is_empty() || text.trim().eq_ignore_ascii_case("q") {
        Ok(None)
    } else {
        Ok(Some(text))
    }
}
fn cancelled(plan: Option<&InstallPlan>) -> Result<Output, Failure> {
    Ok(Output {
        command: "interactive",
        exit_code: 3,
        human: "Cancelled; no writes attempted.\n".into(),
        data: json!({"plan":plan.map(planning::view).transpose()?,"apply_attempted":false,"reason":"cancelled"}),
    })
}
pub(crate) fn execute(
    input: &mut impl BufRead,
    prompts: &mut impl Write,
) -> Result<Output, Failure> {
    write(
        prompts,
        "Interactive selection. Blank, q, or EOF cancels at any prompt. No tools are preselected.\n",
    )?;
    let Some(source) = prompt(input, prompts, "Skill source path: ")? else {
        return cancelled(None);
    };
    let catalog = Catalog::bundled().map_err(Failure::operation)?;
    write(prompts, "Catalog targets and documented scope statuses:\n")?;
    for target in catalog.targets() {
        write(
            prompts,
            &format!(
                "{}: {}\n",
                target.id,
                target
                    .scopes
                    .iter()
                    .map(|scope| format!("{:?}={:?}", scope.scope, scope.status))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )?;
    }
    let Some(targets) = prompt(input, prompts, "Target IDs/aliases (comma separated): ")? else {
        return cancelled(None);
    };
    let targets = targets
        .split(',')
        .map(str::trim)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for target in &targets {
        if catalog.get(target).is_none() {
            return Err(Failure::usage(format!("unknown target: {target}")));
        }
    }
    let Some(scope) = prompt(input, prompts, "Scope (user or project): ")? else {
        return cancelled(None);
    };
    let scope = match scope.trim() {
        "user" => Scope::User,
        "project" => Scope::Project,
        _ => {
            return Err(Failure::usage(
                "unsupported scope; select user or project explicitly",
            ));
        }
    };
    let question = if scope == Scope::User {
        "Explicit user home root: "
    } else {
        "Explicit project root: "
    };
    let Some(root) = prompt(input, prompts, question)? else {
        return cancelled(None);
    };
    let options = PlanArgs {
        source: PathBuf::from(source),
        targets,
        scope,
        user_home: if scope == Scope::User {
            Some(PathBuf::from(&root))
        } else {
            None
        },
        project_root: if scope == Scope::Project {
            Some(PathBuf::from(root))
        } else {
            None
        },
    };
    let plan = planning::build(&options)?;
    write(prompts, &planning::describe(&plan))?;
    if !plan.is_applicable() {
        write(
            prompts,
            "This plan is blocked; confirmation cannot authorize replacement or unsupported behavior.\n",
        )?;
        return installation::execute_plan(plan, false, false);
    }
    let Some(answer) = prompt(
        input,
        prompts,
        "Apply this exact plan? Type yes to approve: ",
    )?
    else {
        return cancelled(Some(&plan));
    };
    if !answer.trim().eq_ignore_ascii_case("yes") {
        return cancelled(Some(&plan));
    }
    // Reuse the exact retained plan shown above. Never rebuild after confirmation.
    installation::execute_plan(plan, true, false)
}

#[cfg(test)]
#[path = "interactive_tests.rs"]
mod tests;
