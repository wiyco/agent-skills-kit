//! Terminal presentation of library-owned permanent-removal plans and outcomes.
use crate::{Failure, Output, args::SelectionArgs, installation::path};
use agent_skills_kit::{
    catalog::Catalog,
    discovery::SkillListing,
    removal::{
        RemovalEntryState, RemovalLimits, RemovalPlan, RemovalRequest, apply_removal, plan_removal,
    },
};
use serde_json::json;
use std::ffi::OsString;

pub(crate) fn plan(
    options: &SelectionArgs,
    listing: SkillListing,
    name: OsString,
) -> Result<RemovalPlan, Failure> {
    let location_index = options
        .location
        .and_then(|n| n.checked_sub(1))
        .ok_or_else(|| Failure::usage("delete requires a displayed location number"))?;
    plan_removal(
        &Catalog::bundled().map_err(Failure::operation)?,
        RemovalRequest {
            listing,
            location_index,
            skill_name: name,
            context: crate::manager::context(options)?,
            limits: RemovalLimits::default(),
        },
    )
    .map_err(Failure::operation)
}
const WARNING: &str = "PERMANENT DELETION: no undo or secure-erasure guarantee. Stop skill writers before confirmation and application. Selecting an agent does not make this skill exclusive to it. Unknown readers may exist.\n";
pub(crate) fn preview(plan: &RemovalPlan, dry_run: bool) -> Output {
    let mut human = String::from(WARNING);
    human.push_str(&format!(
        "Source agent: {} ({:?})\nRemove root: {:?}\n",
        plan.target_id(),
        plan.scope(),
        plan.path()
    ));
    for entry in plan.entries() {
        human.push_str(&format!(
            "Remove {:?}: {:?}, {} bytes, digest {:?}\n",
            entry.relative_path, entry.kind, entry.stamp.length, entry.digest
        ));
    }
    human.push_str(&format!(
        "Total regular-file bytes: {}\nKnown shared readers:\n",
        plan.total_bytes()
    ));
    for reader in plan.readers() {
        human.push_str(&format!(
            "  {} ({:?}): {:?}; native write: {}; ancestors unexpanded: {}\n",
            reader.target_id,
            reader.scope,
            reader.path,
            reader.preferred_write,
            reader.ancestors_unexpanded
        ));
    }
    human.push_str("Unresolved reader evidence (not exhaustive host discovery):\n");
    for reader in plan.unresolved_readers() {
        human.push_str(&format!(
            "  {} ({:?}): {:?}: {}\n",
            reader.target_id, reader.scope, reader.path, reader.reason
        ));
    }
    human.push_str(&format!("No writes attempted. To approve this exact preview, rerun with --confirm-delete {}. Any changed plan is refused.\n", plan.confirmation_token()));
    Output {
        command: "skills.delete",
        exit_code: if dry_run { 0 } else { 3 },
        human,
        data: json!({
            "apply_attempted":false,"dry_run":dry_run,"permanent":true,"warning":WARNING,
            "source_agent":plan.target_id(),"scope":plan.scope(),"root":path(plan.path()),
            "entries":plan.entries().iter().map(|entry| json!({"path":path(&plan.path().join(&entry.relative_path)),"relative_path":path(&entry.relative_path),"kind":format!("{:?}",entry.kind),"bytes":entry.stamp.length,"digest":entry.digest})).collect::<Vec<_>>(),
            "total_bytes":plan.total_bytes(),"confirmation_token":plan.confirmation_token(),
            "known_readers":plan.readers().iter().map(|r|json!({"target_id":r.target_id,"scope":r.scope,"path":path(&r.path),"preferred_write":r.preferred_write,"ancestors_unexpanded":r.ancestors_unexpanded})).collect::<Vec<_>>(),
            "unresolved_readers":plan.unresolved_readers().iter().map(|r|json!({"target_id":r.target_id,"scope":r.scope,"path":r.path.as_deref().map(path),"reason":r.reason})).collect::<Vec<_>>(),
            "exhaustive_host_discovery":false,
        }),
    }
}
fn state(state: &RemovalEntryState) -> &'static str {
    match state {
        RemovalEntryState::Removed => "removed",
        RemovalEntryState::NotAttempted => "not_attempted",
        RemovalEntryState::Remaining => "remaining",
        RemovalEntryState::Unknown => "unknown",
        _ => "unknown",
    }
}
pub(crate) fn execute(
    plan: RemovalPlan,
    confirmation: Option<&str>,
    dry_run: bool,
) -> Result<Output, Failure> {
    let mut output = preview(&plan, dry_run);
    if dry_run || confirmation.is_none() {
        return Ok(output);
    }
    let approved = plan
        .approve_permanent_removal(confirmation.expect("checked confirmation"))
        .map_err(Failure::operation)?;
    let report = apply_removal(approved);
    let successful = report.is_successful();
    // Preserve every outcome even if a diagnostic native path cannot be encoded.
    let failure = |f: &agent_skills_kit::removal::RemovalFailure| json!({"path":path(&f.path),"operation":f.operation,"kind":format!("{:?}",f.kind)});
    output.human = String::from(WARNING);
    for row in &report.outcomes {
        output
            .human
            .push_str(&format!("{} {:?}\n", state(&row.state), row.path));
    }
    if let Some(error) = &report.failure {
        output
            .human
            .push_str(&format!("Failure: {error}; path {:?}\n", error.path));
    }
    if let Some(stage) = &report.retained_stage {
        output.human.push_str(&format!("Retained enclosure diagnostic: {stage:?}; location uncertain: {}. Never use this label as automatic cleanup authority.\n", report.stage_location_uncertain));
    }
    for entry in &report.unrecognized_entries {
        output
            .human
            .push_str(&format!("Unrecognized retained entry: {entry:?}\n"));
    }
    for error in &report.cleanup_failures {
        output.human.push_str(&format!(
            "Cleanup/restoration failure: {error}; path {:?}\n",
            error.path
        ));
    }
    output.human.push_str(&format!("Successful: {successful}; permanent call attempted: {}; restored before removal: {}; unknown-entry scan incomplete: {}\n", report.permanent_removal_attempted, report.restored_before_removal, report.unrecognized_scan_incomplete));
    output.exit_code = if successful { 0 } else { 4 };
    output.data["apply_attempted"] = json!(true);
    output.data["report"] = json!({
        "successful":successful,"original_path":path(&report.original_path),
        "outcomes":report.outcomes.iter().map(|r|json!({"path":path(&r.path),"state":state(&r.state),"failure":r.failure.as_ref().map(failure)})).collect::<Vec<_>>(),
        "failure":report.failure.as_ref().map(failure),"permanent_removal_attempted":report.permanent_removal_attempted,
        "retained_stage":report.retained_stage.as_deref().map(path),"stage_location_uncertain":report.stage_location_uncertain,
        "unrecognized_entries":report.unrecognized_entries.iter().map(|p|path(p)).collect::<Vec<_>>(),"unrecognized_scan_incomplete":report.unrecognized_scan_incomplete,
        "restored_before_removal":report.restored_before_removal,"cleanup_failures":report.cleanup_failures.iter().map(failure).collect::<Vec<_>>(),
    });
    Ok(output)
}
