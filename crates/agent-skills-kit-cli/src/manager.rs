use crate::{Failure, Output, absolute, args::SelectionArgs, installation};
use agent_skills_kit::{
    catalog::Catalog,
    discovery::{DiscoveryLimits, SkillListing, list_skills},
    planner::{InstallPlan, InstallRequest, PlanLimits, plan_install},
    receipt::NativePath,
    resolver::{InstallContext, native_platform},
    validation::ValidatedSkill,
};
use serde_json::json;
use std::ffi::OsStr;

pub(crate) fn context(options: &SelectionArgs) -> Result<InstallContext, Failure> {
    let mut context = InstallContext::new(
        native_platform().ok_or_else(|| Failure::operation("unsupported native platform"))?,
    );
    context.user_home = options.user_home.clone().map(absolute).transpose()?;
    context.project_root = options.project_root.clone().map(absolute).transpose()?;
    Ok(context)
}
pub(crate) fn discover(options: &SelectionArgs) -> Result<SkillListing, Failure> {
    list_skills(
        &Catalog::bundled().map_err(Failure::operation)?,
        &options.agent,
        options.scope,
        &context(options)?,
        DiscoveryLimits::default(),
    )
    .map_err(Failure::operation)
}
pub(crate) fn render(listing: &SkillListing) -> Result<Output, Failure> {
    let path = |p: &std::path::Path| NativePath::from_native(p).map_err(Failure::operation);
    let mut human = format!(
        "Existing skills for {} ({:?}). Placement is not exclusive.\n",
        listing.target_id, listing.scope
    );
    let mut locations = vec![];
    let mut incomplete = false;
    for (index, root) in listing.locations.iter().enumerate() {
        human.push_str(&format!(
            "Location {}: {:?} [{}]\n",
            index + 1,
            root.path,
            if root.preferred_write {
                "native write"
            } else {
                "compatibility reader"
            }
        ));
        if root.missing {
            human.push_str("  Missing; no directory created.\n");
        }
        if let Some(alias) = &root.alias_of {
            human.push_str(&format!(
                "  Same physical root as {alias:?}; not scanned twice.\n"
            ));
        }
        if let Some(problem) = &root.problem {
            incomplete = true;
            human.push_str(&format!("  Incomplete: {problem}\n"));
        }
        let mut skills = vec![];
        for row in &root.skills {
            let error = row.inspection.as_ref().err().map(ToString::to_string);
            human.push_str(&format!(
                "  {:?}: {}\n",
                row.name,
                error.as_deref().unwrap_or("validated inert skill")
            ));
            skills.push(json!({"path":path(&row.path)?, "name":NativePath::from_native(std::path::Path::new(&row.name)).map_err(Failure::operation)?, "selectable":root.problem.is_none() && error.is_none(), "error":error}));
        }
        locations.push(json!({"number":index+1,"path":path(&root.path)?,"preferred_write":root.preferred_write,"reader_roles":root.discovery,"missing":root.missing,"alias_of":root.alias_of.as_deref().map(path).transpose()?,"problem":root.problem.as_ref().map(ToString::to_string),"skills":skills}));
    }
    human.push_str(
        "Only the explicitly selected roots were inspected; ancestor searches are not expanded.\n",
    );
    for warning in &listing.warnings {
        human.push_str(&format!("Warning: {warning:?}\n"));
    }
    Ok(Output {
        command: "skills.list",
        exit_code: if incomplete { 2 } else { 0 },
        human,
        data: json!({"target_id":listing.target_id,"scope":listing.scope,"locations":locations,"unexpanded_ancestors":listing.unexpanded_ancestors.len(),"warnings":listing.warnings.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>(),"complete":!incomplete,"exhaustive_host_discovery":false}),
    })
}
pub(crate) fn select(
    listing: &SkillListing,
    number: usize,
    name: &OsStr,
) -> Result<ValidatedSkill, Failure> {
    let root = number
        .checked_sub(1)
        .and_then(|index| listing.locations.get(index))
        .ok_or_else(|| Failure::usage("unknown displayed location number"))?;
    if root.problem.is_some() || root.missing || root.alias_of.is_some() {
        return Err(Failure::usage(
            "select a complete, present, non-alias location",
        ));
    }
    let snapshot = root
        .snapshot()
        .ok_or_else(|| Failure::operation("listing has no retained namespace preconditions"))?;
    if !snapshot.is_current().map_err(Failure::operation)? {
        return Err(Failure::operation("listing namespace changed; list again"));
    }
    let row = root
        .skills
        .iter()
        .find(|row| row.name == name)
        .ok_or_else(|| Failure::usage("skill name is not present in the selected location"))?;
    row.inspection
        .as_ref()
        .map(|skill| skill.as_ref().clone())
        .map_err(Failure::operation)
}
pub(crate) fn plan(
    options: &SelectionArgs,
    selected: ValidatedSkill,
    targets: &[String],
) -> Result<InstallPlan, Failure> {
    let context = context(options)?;
    let skills = [selected];
    let targets = targets.iter().map(String::as_str).collect::<Vec<_>>();
    plan_install(
        &Catalog::bundled().map_err(Failure::operation)?,
        InstallRequest {
            skills: &skills,
            target_ids: &targets,
            scope: options.scope,
            context: &context,
            limits: PlanLimits::default(),
        },
    )
    .map_err(Failure::operation)
}
pub(crate) fn execute(
    options: SelectionArgs,
    copy: Option<(std::ffi::OsString, Vec<String>, bool, bool)>,
) -> Result<Output, Failure> {
    let listing = discover(&options)?;
    if let Some((name, targets, yes, dry_run)) = copy {
        let selected = select(
            &listing,
            options
                .location
                .ok_or_else(|| Failure::usage("copy requires a location"))?,
            &name,
        )?;
        let plan = plan(&options, selected, &targets)?;
        let mut output = installation::execute_plan(plan, yes, dry_run)?;
        output.command = "skills.copy";
        output.data["source_agent"] = json!(listing.target_id);
        Ok(output)
    } else {
        render(&listing)
    }
}

pub(crate) fn interactive(
    input: &mut impl std::io::BufRead,
    prompts: &mut impl std::io::Write,
) -> Result<Output, Failure> {
    use crate::interactive::{prompt, write};
    use agent_skills_kit::catalog::Scope;
    let cancelled = || Output {
        command: "manage",
        exit_code: 3,
        human: "Cancelled; no writes attempted.\n".into(),
        data: json!({"apply_attempted":false,"reason":"cancelled"}),
    };
    let catalog = Catalog::bundled().map_err(Failure::operation)?;
    write(
        prompts,
        "Select source agent first. Blank, q, or EOF cancels.\n",
    )?;
    for target in catalog.targets() {
        write(prompts, &format!("{}\n", target.id))?;
    }
    let Some(agent) = prompt(input, prompts, "Source agent ID/alias: ")? else {
        return Ok(cancelled());
    };
    if catalog.get(agent.trim()).is_none() {
        return Err(Failure::usage("unknown source agent"));
    }
    let Some(scope) = prompt(input, prompts, "Scope (user or project): ")? else {
        return Ok(cancelled());
    };
    let scope = match scope.trim() {
        "user" => Scope::User,
        "project" => Scope::Project,
        _ => return Err(Failure::usage("select user or project explicitly")),
    };
    let Some(root) = prompt(
        input,
        prompts,
        if scope == Scope::User {
            "Explicit user home root: "
        } else {
            "Explicit project root: "
        },
    )?
    else {
        return Ok(cancelled());
    };
    let mut options = SelectionArgs {
        agent: agent.trim().into(),
        scope,
        user_home: None,
        project_root: None,
        location: None,
    };
    if scope == Scope::User {
        options.user_home = Some(root.into());
    } else {
        options.project_root = Some(root.into());
    }
    let listing = discover(&options)?;
    write(prompts, &render(&listing)?.human)?;
    let Some(location) = prompt(input, prompts, "Source location number: ")? else {
        return Ok(cancelled());
    };
    let location = location
        .trim()
        .parse::<usize>()
        .map_err(|_| Failure::usage("select a displayed location number"))?;
    let Some(name) = prompt(input, prompts, "Skill name from this location: ")? else {
        return Ok(cancelled());
    };
    let selected = select(&listing, location, OsStr::new(&name))?;
    let Some(operation) = prompt(input, prompts, "Operation (copy): ")? else {
        return Ok(cancelled());
    };
    if operation.trim() != "copy" {
        return Err(Failure::usage(
            "only copy is available; permanent deletion follows its separate safety implementation",
        ));
    }
    let Some(targets) = prompt(
        input,
        prompts,
        "Destination agent IDs/aliases (comma separated): ",
    )?
    else {
        return Ok(cancelled());
    };
    let targets = targets
        .split(',')
        .map(|x| x.trim().to_owned())
        .collect::<Vec<_>>();
    let plan = plan(&options, selected, &targets)?;
    write(prompts, &crate::planning::describe(&plan))?;
    if !plan.is_applicable() {
        return installation::execute_plan(plan, false, false);
    }
    let Some(answer) = prompt(
        input,
        prompts,
        "Copy this exact plan? Type yes to approve: ",
    )?
    else {
        return Ok(cancelled());
    };
    if !answer.trim().eq_ignore_ascii_case("yes") {
        return Ok(cancelled());
    }
    let mut output = installation::execute_plan(plan, true, false)?;
    output.command = "manage";
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixture as fixture;
    use std::{fs, io::Cursor};
    fn setup() -> (fixture::Fixture, std::path::PathBuf) {
        let fixture = fixture::Fixture::new();
        let home = fs::canonicalize(&fixture.0).unwrap().join("home");
        let source = fixture.source(OsStr::new("home/.agents/skills"));
        assert!(fs::canonicalize(source).unwrap().starts_with(&home));
        (fixture, home)
    }
    #[test]
    fn host_first_copy_prompts_show_list_and_preview_before_exact_confirmation() {
        let (_fixture, home) = setup();
        let answers = format!(
            "codex\nuser\n{}\n1\nsample\ncopy\nclaude-code\nyes\n",
            home.display()
        );
        let mut prompts = vec![];
        let output = interactive(&mut Cursor::new(answers), &mut prompts).unwrap();
        assert_eq!(output.exit_code, 0, "{output:?}");
        assert_eq!(
            fs::read(home.join(".claude/skills/sample/resource.ps1")).unwrap(),
            b"inert resource; never executed"
        );
        let text = String::from_utf8(prompts).unwrap();
        assert!(text.find("Source agent ID").unwrap() < text.find("Scope (").unwrap());
        assert!(
            text.find("validated inert skill").unwrap() < text.find("Skill name from").unwrap()
        );
        assert!(text.find("create ").unwrap() < text.find("Copy this exact plan?").unwrap());
    }
    #[test]
    fn every_manager_prompt_cancels_on_blank_q_or_eof_without_copying() {
        for stop in 0..8 {
            for ending in ["", "\n", "q\n"] {
                let (_fixture, home) = setup();
                let root = home.to_str().unwrap();
                let answers = [
                    "codex",
                    "user",
                    root,
                    "1",
                    "sample",
                    "copy",
                    "claude-code",
                    "yes",
                ];
                let text = answers[..stop]
                    .iter()
                    .map(|x| format!("{x}\n"))
                    .collect::<String>()
                    + ending;
                let output = interactive(&mut Cursor::new(text), &mut vec![]).unwrap();
                assert_eq!(output.exit_code, 3, "stop={stop}: {output:?}");
                assert_eq!(output.data["apply_attempted"], false);
                assert!(!home.join(".claude").exists());
            }
        }
    }
    #[test]
    fn selected_inventory_cannot_be_replaced_by_new_contents_before_planning() {
        let (_fixture, home) = setup();
        let options = SelectionArgs {
            agent: "codex".into(),
            scope: agent_skills_kit::catalog::Scope::User,
            user_home: Some(home.clone()),
            project_root: None,
            location: Some(1),
        };
        let listing = discover(&options).unwrap();
        let selected = select(&listing, 1, OsStr::new("sample")).unwrap();
        fs::write(
            home.join(".agents/skills/sample/resource.ps1"),
            b"changed after selection",
        )
        .unwrap();
        assert!(plan(&options, selected, &["claude-code".into()]).is_err());
        assert!(!home.join(".claude").exists());
        drop(listing);
    }
}
