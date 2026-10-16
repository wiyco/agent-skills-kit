use crate::{Failure, Output, args::PlanArgs, planning};
use agent_skills_kit::{
    executor::{ApplyOptions, InstallReport, OutcomeStatus, apply_plan},
    planner::InstallPlan,
    receipt::{NativePath, OperationId, Receipt, ReceiptLimits},
};
use serde_json::{Value, json};
use std::path::Path;

fn path(path: &Path) -> Value {
    match NativePath::from_native(path) {
        Ok(native) => json!(native),
        // Never discard committed outcomes because a diagnostic path cannot be encoded.
        Err(error) => {
            json!({"encoding_error": error.to_string(), "debug_label": format!("{path:?}")})
        }
    }
}
fn status(status: &OutcomeStatus) -> &'static str {
    match status {
        OutcomeStatus::Created => "created",
        OutcomeStatus::Unchanged => "unchanged",
        OutcomeStatus::Failed(_) => "failed",
        OutcomeStatus::CommittedWithPermissionFailure(_) => "committed_with_permission_failure",
        OutcomeStatus::NotAttempted => "not_attempted",
        _ => "unknown",
    }
}
fn report_view(report: &InstallReport) -> Value {
    json!({
        "successful": report.is_successful(),
        "preflight_error": report.preflight_error.as_ref().map(ToString::to_string),
        "outcomes": report.outcomes.iter().map(|row| json!({
            "path": row.path.as_deref().map(path),
            "attributions": row.attributions.iter().map(|item| json!({"source":path(&item.source),"target_id":item.target_id})).collect::<Vec<_>>(),
            "status": status(&row.status), "payload_was_committed": row.status.payload_was_committed(),
            "error": match &row.status {OutcomeStatus::Failed(error) | OutcomeStatus::CommittedWithPermissionFailure(error) => Some(format!("{error:?}")), _ => None},
        })).collect::<Vec<_>>(),
        "cleanup_errors": report.cleanup_errors.iter().map(|error| json!({"path":path(&error.path),"message":error.to_string()})).collect::<Vec<_>>(),
    })
}
pub(crate) fn execute(options: PlanArgs, yes: bool, dry_run: bool) -> Result<Output, Failure> {
    let plan = planning::build(&options)?;
    execute_plan(plan, yes, dry_run)
}
pub(crate) fn execute_plan(plan: InstallPlan, yes: bool, dry_run: bool) -> Result<Output, Failure> {
    // Prepare the immutable preview representation before permitting any writes.
    let preview = planning::view(&plan)?;
    let description = planning::describe(&plan);
    if dry_run || !plan.is_applicable() || !yes {
        let exit_code = if !plan.is_applicable() {
            2
        } else if dry_run {
            0
        } else {
            3
        };
        let message = match exit_code {
            0 => "Dry-run; no writes attempted.",
            3 => {
                "Approval required; no writes attempted. Rerun with --yes to approve the freshly derived plan."
            }
            _ => {
                "Plan is blocked; no writes attempted. --yes never permits overwrite or safety bypass."
            }
        };
        return Ok(Output {
            command: "install",
            exit_code,
            human: format!("{message}\n{description}"),
            data: json!({"plan":preview,"dry_run":dry_run,"approval_required":exit_code==3,"apply_attempted":false,"report":null,"receipt":null,"receipt_error":null}),
        });
    }
    let operation_id = OperationId::new("cli-install").map_err(Failure::operation)?;
    let report = apply_plan(&plan, ApplyOptions::default());
    // A correlation label, not a unique ownership token or persistence instruction.
    let receipt = Receipt::from_report(&plan, &report, operation_id, ReceiptLimits::default())
        .and_then(|receipt| receipt.to_json(ReceiptLimits::default()))
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            serde_json::from_slice::<Value>(&bytes).map_err(|error| error.to_string())
        });
    Ok(complete(preview, description, report, receipt))
}
fn complete(
    preview: Value,
    description: String,
    report: InstallReport,
    receipt: Result<Value, String>,
) -> Output {
    let (receipt, receipt_error) = match receipt {
        Ok(receipt) => (Some(receipt), None),
        Err(error) => (None, Some(error)),
    };
    let successful = report.is_successful() && receipt_error.is_none();
    let mut human = format!("Approved derived plan:\n{description}Installation outcomes:\n");
    for row in &report.outcomes {
        human.push_str(&format!("{} {:?}\n", status(&row.status), row.path));
        if let OutcomeStatus::Failed(error) | OutcomeStatus::CommittedWithPermissionFailure(error) =
            &row.status
        {
            human.push_str(&format!("  {error:?}\n"));
        }
    }
    if let Some(error) = &report.preflight_error {
        human.push_str(&format!("Preflight: {error}\n"));
    }
    for error in &report.cleanup_errors {
        human.push_str(&format!("Cleanup needs review: {error:?}\n"));
    }
    if let Some(error) = &receipt_error {
        human.push_str(&format!(
            "Receipt unavailable; preserve outcomes: {error}\n"
        ));
    }
    if !successful {
        human.push_str("Review committed outcomes and cleanup diagnostics before retrying; no global rollback or implicit permission repair occurs.\n");
    }
    Output {
        command: "install",
        exit_code: if successful { 0 } else { 4 },
        human,
        data: json!({"plan":preview,"dry_run":false,"approval_required":false,"apply_attempted":true,"report":report_view(&report),"receipt":receipt,"receipt_error":receipt_error}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_skills_kit::executor::{ApplyError, ApplyErrorKind, DestinationOutcome};
    #[test]
    fn mixed_committed_failures_and_cleanup_diagnostics_remain_visible() {
        // Synthetic report: codec coverage, not a native permission-failure test.
        let label = std::env::temp_dir().join("synthetic-cli-report");
        let failure = ApplyError {
            path: label.clone(),
            kind: ApplyErrorKind::Io {
                operation: "synthetic permission finalization",
                kind: std::io::ErrorKind::PermissionDenied,
                raw_os_error: Some(5),
            },
        };
        let report = InstallReport {
            preflight_error: None,
            cleanup_errors: vec![failure.clone()],
            outcomes: [
                OutcomeStatus::Created,
                OutcomeStatus::CommittedWithPermissionFailure(failure),
                OutcomeStatus::NotAttempted,
            ]
            .into_iter()
            .map(|status| DestinationOutcome {
                path: Some(label.clone()),
                attributions: vec![],
                status,
            })
            .collect(),
        };
        let value = report_view(&report);
        assert_eq!(value["successful"], false);
        assert_eq!(value["outcomes"][0]["payload_was_committed"], true);
        assert_eq!(
            value["outcomes"][1]["status"],
            "committed_with_permission_failure"
        );
        assert_eq!(value["outcomes"][1]["payload_was_committed"], true);
        assert!(
            value["outcomes"][1]["error"]
                .as_str()
                .unwrap()
                .contains("synthetic permission finalization")
        );
        assert_eq!(value["outcomes"][2]["status"], "not_attempted");
        assert_eq!(value["outcomes"][2]["payload_was_committed"], false);
        assert_eq!(value["cleanup_errors"].as_array().unwrap().len(), 1);
    }
    #[test]
    fn receipt_encoding_failure_never_discards_successful_committed_outcomes() {
        // Deliberately synthetic failure of the pure presentation boundary.
        let report = InstallReport {
            preflight_error: None,
            cleanup_errors: vec![],
            outcomes: vec![DestinationOutcome {
                path: Some(std::env::temp_dir().join("synthetic-created")),
                attributions: vec![],
                status: OutcomeStatus::Created,
            }],
        };
        let output = complete(
            json!({"synthetic":true}),
            "Synthetic preview".into(),
            report,
            Err("synthetic receipt budget failure".into()),
        );
        assert_eq!(output.exit_code, 4);
        assert_eq!(output.data["report"]["successful"], true);
        assert_eq!(output.data["report"]["outcomes"][0]["status"], "created");
        assert_eq!(
            output.data["report"]["outcomes"][0]["payload_was_committed"],
            true
        );
        assert!(output.data["receipt"].is_null());
        assert_eq!(
            output.data["receipt_error"],
            "synthetic receipt budget failure"
        );
        assert!(
            output
                .human
                .contains("Receipt unavailable; preserve outcomes")
        );
    }
}
