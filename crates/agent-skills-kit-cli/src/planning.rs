use crate::{Failure, Output, absolute, args::PlanArgs};
use agent_skills_kit::{
    catalog::Catalog,
    planner::{
        DestinationProblem, InstallPlan, InstallRequest, IntendedWriteKind, PlanLimits,
        PlannedAction, plan_install,
    },
    receipt::NativePath,
    resolver::{InstallContext, ResolutionWarning, native_platform},
    validation::{InventoryLimits, inventory_skill},
};
use serde_json::{Value, json};

pub(crate) fn build(options: &PlanArgs) -> Result<InstallPlan, Failure> {
    let platform =
        native_platform().ok_or_else(|| Failure::operation("unsupported native platform"))?;
    let mut context = InstallContext::new(platform);
    context.user_home = options.user_home.clone().map(absolute).transpose()?;
    context.project_root = options.project_root.clone().map(absolute).transpose()?;
    let sources = [inventory_skill(
        &absolute(options.source.clone())?,
        InventoryLimits::default(),
    )
    .map_err(Failure::operation)?];
    let targets = options
        .targets
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    plan_install(
        &Catalog::bundled().map_err(Failure::operation)?,
        InstallRequest {
            skills: &sources,
            target_ids: &targets,
            scope: options.scope,
            context: &context,
            limits: PlanLimits::default(),
        },
    )
    .map_err(Failure::operation)
}
fn action(action: PlannedAction) -> &'static str {
    match action {
        PlannedAction::Create => "create",
        PlannedAction::Unchanged => "unchanged",
        PlannedAction::Conflict => "conflict",
        PlannedAction::Unsupported => "unsupported",
        _ => "unknown",
    }
}
fn warning(warning: &ResolutionWarning) -> String {
    match warning {
        ResolutionWarning::PlacementNotExclusive => {
            "Placement is not exclusive: compatible hosts may discover this location.".into()
        }
        ResolutionWarning::HostDiscoveryUntested => {
            "Real host discovery has not been tested.".into()
        }
        ResolutionWarning::ResolverEvidenceAbsent => "Resolver test evidence is absent.".into(),
        ResolutionWarning::AncestorDiscoveryNotExpanded => {
            "Documented ancestor discovery is not expanded into filesystem results.".into()
        }
        ResolutionWarning::DocumentedLimitation(text) => text.clone(),
        _ => format!("Unrecognized library warning: {warning:?}"),
    }
}
pub(crate) fn view(plan: &InstallPlan) -> Result<Value, Failure> {
    let path = |path: &std::path::Path| NativePath::from_native(path).map_err(Failure::operation);
    let sources = plan.sources().iter().map(|source| Ok(json!({
        "path": path(source.inventory().requested_root())?, "name": source.metadata().name(),
        "resource_entries": source.inventory().entries().len(), "total_bytes": source.inventory().total_bytes(),
    }))).collect::<Result<Vec<_>, Failure>>()?;
    let targets = plan.targets().iter().map(|target| {
        let resolved = target.resolution().ok();
        let discovery = resolved.map(|resolved| resolved.discovery.iter().map(|location| path(&location.location.path)).collect::<Result<Vec<_>,_>>()).transpose()?.unwrap_or_default();
        Ok(json!({
            "id": target.id(), "preferred_write": resolved.map(|value| path(&value.preferred_write.path)).transpose()?,
            "discovery": discovery, "warnings": target.warnings().iter().map(warning).collect::<Vec<_>>(),
            "resolution_error": target.resolution().err().map(ToString::to_string),
        }))
    }).collect::<Result<Vec<_>, Failure>>()?;
    let destinations = plan.destinations().iter().map(|row| Ok(json!({
        "path": row.path().map(path).transpose()?, "action": action(row.action()),
        "attributions": row.attributions().iter().map(|attribution| json!({"source_index": attribution.source_index, "target_index": attribution.target_index})).collect::<Vec<_>>(),
        "problems": row.problems().iter().map(|problem| format!("{problem:?}")).collect::<Vec<_>>(),
    }))).collect::<Result<Vec<_>, Failure>>()?;
    let writes = plan.intended_writes().iter().map(|write| Ok(json!({
        "path": path(&write.path)?, "kind": match write.kind {
            IntendedWriteKind::CreateParent => "create_parent", IntendedWriteKind::CreateSkillRoot => "create_skill_root",
            IntendedWriteKind::CreateDirectory => "create_directory", IntendedWriteKind::CopyFile => "copy_file", _ => "unknown",
        }, "mode": write.mode, "bytes": write.bytes,
        "digest_blake3_256": write.digest.map(|digest| digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>()),
    }))).collect::<Result<Vec<_>, Failure>>()?;
    Ok(
        json!({"scope": plan.scope(), "applicable": plan.is_applicable(), "sources": sources,
        "targets": targets, "destinations": destinations, "intended_writes": writes, "inspected_bytes": plan.inspected_bytes()}),
    )
}
fn problem_message(problem: &DestinationProblem) -> String {
    match problem {
        DestinationProblem::ExistingContentDiffers => {
            "Existing content differs; no overwrite is permitted.".into()
        }
        DestinationProblem::Target(error) => format!("Target selection is unsupported: {error}"),
        DestinationProblem::Host(error) => format!("Skill is unsupported by this host: {error}"),
        DestinationProblem::Namespace(error) => {
            format!("Destination cannot be safely inspected: {error}")
        }
        DestinationProblem::ExistingInspection(error) => {
            format!("Existing payload cannot be fully validated: {error}")
        }
        DestinationProblem::ExistingNameMismatch => {
            "Existing directory spelling differs; replacement is refused.".into()
        }
        DestinationProblem::CompetingSources => {
            "Selected sources conflict at one physical destination.".into()
        }
        DestinationProblem::PortableNameCollision { first, second } => {
            format!("Portable names collide: {first:?} and {second:?}")
        }
        _ => format!("Unsupported plan condition: {problem:?}"),
    }
}
pub(crate) fn describe(plan: &InstallPlan) -> String {
    let mut human = String::from("Plan placement is not exclusive.\n");
    let mut groups: Vec<(String, Vec<&str>)> = vec![];
    let mut positions = std::collections::BTreeMap::new();
    for target in plan.targets() {
        for message in target.warnings() {
            let message = warning(message);
            let index = *positions.entry(message.clone()).or_insert_with(|| {
                groups.push((message, vec![]));
                groups.len() - 1
            });
            groups[index].1.push(target.id());
        }
    }
    for (message, targets) in groups {
        human.push_str(&format!("{}: {message}\n", targets.join(", ")));
    }
    human.push_str("Destinations:\n");
    for destination in plan.destinations() {
        let label = destination.path().map_or_else(
            || "unresolved destination".into(),
            |path| format!("{path:?}"),
        );
        human.push_str(&format!("{} {label}\n", action(destination.action())));
        for problem in destination.problems() {
            human.push_str(&format!("  {}\n", problem_message(problem)));
        }
    }
    human
}
pub(crate) fn execute(options: PlanArgs) -> Result<Output, Failure> {
    let plan = build(&options)?;
    let human = format!(
        "Read-only plan; no destinations written.\n{}",
        describe(&plan)
    );
    Ok(Output {
        command: "plan",
        exit_code: if plan.is_applicable() { 0 } else { 2 },
        data: view(&plan)?,
        human,
    })
}
