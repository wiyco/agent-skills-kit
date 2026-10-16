#![cfg(any(target_os = "linux", target_os = "macos"))]
use agent_skills_kit::{
    catalog::{Catalog, Scope},
    executor::*,
    planner::*,
    receipt::*,
    resolver::{InstallContext, native_platform},
    validation::{InventoryLimits, inventory_skill},
};
use std::{
    fs,
    io::Write,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = (0..100)
            .find_map(|_| {
                let path = std::env::temp_dir().join(format!(
                    "ask-receipt-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&path) {
                    Ok(()) => Some(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                    Err(error) => panic!("owned receipt fixture: {error}"),
                }
            })
            .expect("exclusive fixture collision budget");
        fs::create_dir_all(root.join("source/sample/empty")).unwrap();
        fs::create_dir(root.join("home")).unwrap();
        fs::write(root.join("source/sample/SKILL.md"),b"---\nname: sample\ndescription: original receipt fixture\ncustom: retained\n---\nInert data.\n").unwrap();
        fs::write(root.join("source/sample/resource"), b"original bytes").unwrap();
        Self(root)
    }
    fn source(&self) -> PathBuf {
        self.0.join("source/sample")
    }
    fn destination(&self) -> PathBuf {
        self.0.join("home/.agents/skills/sample")
    }
    fn plan(&self, targets: &[&str]) -> InstallPlan {
        let sources = [inventory_skill(&self.source(), InventoryLimits::default()).unwrap()];
        let mut context = InstallContext::new(native_platform().unwrap());
        context.user_home = Some(self.0.join("home"));
        plan_install(
            &Catalog::bundled().unwrap(),
            InstallRequest {
                skills: &sources,
                target_ids: targets,
                scope: Scope::User,
                context: &context,
                limits: PlanLimits::default(),
            },
        )
        .unwrap()
    }
    fn existing(&self) {
        fs::create_dir_all(self.destination().join("empty")).unwrap();
        for name in ["SKILL.md", "resource"] {
            fs::copy(self.source().join(name), self.destination().join(name)).unwrap();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("owned receipt fixture teardown");
    }
}
fn receipt(plan: &InstallPlan, report: &InstallReport, id: &str) -> Receipt {
    Receipt::from_report(
        plan,
        report,
        OperationId::new(id).unwrap(),
        ReceiptLimits::default(),
    )
    .unwrap()
}
fn bytes(root: &Path) -> Vec<(PathBuf, Vec<u8>, u32)> {
    fn walk(root: &Path, path: &Path, output: &mut Vec<(PathBuf, Vec<u8>, u32)>) {
        let meta = fs::symlink_metadata(path).unwrap();
        let data = if meta.is_file() {
            fs::read(path).unwrap()
        } else {
            Vec::new()
        };
        output.push((
            path.strip_prefix(root).unwrap().to_owned(),
            data,
            meta.mode(),
        ));
        if meta.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                walk(root, &entry.unwrap().path(), output);
            }
        }
    }
    let mut output = Vec::new();
    walk(root, root, &mut output);
    output.sort_by(|a, b| a.0.cmp(&b.0));
    output
}

#[test]
fn real_receipts_are_explicitly_stored_outside_payloads_and_rerun_is_unchanged() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&["codex", "codex-cli"]);
    let report = apply_plan(&plan, ApplyOptions::default());
    assert!(report.is_successful(), "{report:#?}");
    let before = bytes(&fixture.0);
    let receipt = receipt(&plan, &report, "first-operation");
    assert_eq!(bytes(&fixture.0), before);
    assert_eq!(receipt.document().destinations.len(), 1);
    assert_eq!(receipt.document().targets, ["codex"]);
    assert_eq!(
        receipt.document().destinations[0].status,
        ReceiptStatus::Created
    );
    assert_eq!(receipt.document().sources[0].entries.len(), 3);
    let manifest = receipt.document().sources[0]
        .entries
        .iter()
        .find(|entry| entry.path.to_native().unwrap() == Path::new("SKILL.md"))
        .unwrap();
    assert_eq!(
        manifest.digest.as_ref().unwrap().as_str(),
        ContentDigest::from_bytes(
            plan.sources()[0]
                .inventory()
                .entries()
                .iter()
                .find(|entry| entry.relative_path == Path::new("SKILL.md"))
                .unwrap()
                .digest
                .as_ref()
                .unwrap()
        )
        .as_str()
    );
    assert!(!fixture.0.join("receipts").exists());
    assert!(!fixture.destination().join("receipt.json").exists());
    // Persistence is an explicit caller operation, not receipt/library behavior.
    fs::create_dir(fixture.0.join("receipts")).unwrap();
    let stored = fixture.0.join("receipts/first.json");
    let encoded = receipt.to_json(ReceiptLimits::default()).unwrap();
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stored)
        .unwrap();
    file.write_all(&encoded).unwrap();
    assert_eq!(
        Receipt::from_json(&fs::read(&stored).unwrap(), ReceiptLimits::default()).unwrap(),
        receipt
    );
    let inode = fs::metadata(fixture.destination()).unwrap().ino();
    let plan = fixture.plan(&["codex"]);
    let before = bytes(&fixture.0);
    let report = apply_plan(&plan, ApplyOptions::default());
    let repeated = Receipt::from_report(
        &plan,
        &report,
        OperationId::new("second-operation").unwrap(),
        ReceiptLimits::default(),
    )
    .unwrap();
    assert_eq!(
        repeated.document().destinations[0].planned_action,
        ReceiptPlanAction::Unchanged
    );
    assert_eq!(
        repeated.document().destinations[0].status,
        ReceiptStatus::Unchanged
    );
    assert_eq!(fs::metadata(fixture.destination()).unwrap().ino(), inode);
    assert_eq!(bytes(&fixture.0), before);
}

#[test]
fn preexisting_unmanaged_content_is_not_claimed_as_newly_created() {
    let fixture = Fixture::new();
    fixture.existing();
    let plan = fixture.plan(&["codex"]);
    let inode = fs::metadata(fixture.destination()).unwrap().ino();
    let before = bytes(&fixture.0);
    let mut report = apply_plan(&plan, ApplyOptions::default());
    assert_eq!(report.outcomes[0].status, OutcomeStatus::Unchanged);
    assert_eq!(
        receipt(&plan, &report, "unmanaged-existing")
            .document()
            .destinations[0]
            .status,
        ReceiptStatus::Unchanged
    );
    report.outcomes[0].status = OutcomeStatus::Created;
    assert!(
        Receipt::from_report(
            &plan,
            &report,
            OperationId::new("false-claim").unwrap(),
            ReceiptLimits::default()
        )
        .is_err()
    );
    assert_eq!(fs::metadata(fixture.destination()).unwrap().ino(), inode);
    assert_eq!(bytes(&fixture.0), before);
}

#[test]
fn stale_or_corrupt_saved_receipts_do_not_replace_manually_edited_content() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&["codex"]);
    let report = apply_plan(&plan, ApplyOptions::default());
    let saved = receipt(&plan, &report, "old-operation")
        .to_json(ReceiptLimits::default())
        .unwrap();
    fs::write(
        fixture.destination().join("resource"),
        b"manual edit must remain",
    )
    .unwrap();
    let before = bytes(&fixture.0);
    let stale = Receipt::from_json(&saved, ReceiptLimits::default()).unwrap();
    assert_eq!(
        stale.document().destinations[0].status,
        ReceiptStatus::Created
    );
    let plan = fixture.plan(&["codex"]);
    assert_eq!(plan.destinations()[0].action(), PlannedAction::Conflict);
    let report = apply_plan(&plan, ApplyOptions::default());
    assert!(report.preflight_error.is_some());
    let current = receipt(&plan, &report, "conflict-retry");
    assert_eq!(
        current.document().destinations[0].status,
        ReceiptStatus::NotAttempted
    );
    assert_eq!(
        current.document().destinations[0].planned_action,
        ReceiptPlanAction::Conflict
    );
    assert_eq!(
        current.document().destinations[0].plan_diagnostics[0].code,
        ReceiptFailureCode::ExistingContentDiffers
    );
    for corrupt in [b"{".as_slice(), br#"{"schema_version":99}"#, b"not json"] {
        assert!(Receipt::from_json(corrupt, ReceiptLimits::default()).is_err());
    }
    assert_eq!(
        fs::read(fixture.destination().join("resource")).unwrap(),
        b"manual edit must remain"
    );
    assert_eq!(bytes(&fixture.0), before);
}

#[test]
fn receipt_failure_keeps_full_report_and_successful_files_available() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&["codex"]);
    let report = apply_plan(&plan, ApplyOptions::default());
    let before = bytes(&fixture.0);
    assert!(
        Receipt::from_report(
            &plan,
            &report,
            OperationId::new("small-bounds").unwrap(),
            ReceiptLimits {
                max_path_units: 1,
                ..ReceiptLimits::default()
            }
        )
        .is_err()
    );
    let valid = receipt(&plan, &report, "encoding-limit");
    assert_eq!(
        valid
            .to_json(ReceiptLimits {
                max_document_bytes: 1,
                ..ReceiptLimits::default()
            })
            .unwrap_err(),
        ReceiptError::Limit("JSON bytes")
    );
    assert!(report.is_successful());
    assert_eq!(report.outcomes[0].status, OutcomeStatus::Created);
    assert_eq!(bytes(&fixture.0), before);
}

#[test]
fn mismatched_report_labels_attribution_and_counts_are_rejected_without_io() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&["codex"]);
    let report = apply_plan(&plan, ApplyOptions::default());
    let before = bytes(&fixture.0);
    for mutation in 0..4 {
        let mut changed = report.clone();
        match mutation {
            0 => {
                changed.outcomes.clear();
            }
            1 => changed.outcomes[0].attributions[0].target_id = "wrong-target".into(),
            2 => changed.outcomes[0].path.as_mut().unwrap().push("."),
            3 => changed.outcomes[0].attributions[0].source.push("."),
            _ => unreachable!(),
        }
        assert!(
            Receipt::from_report(
                &plan,
                &changed,
                OperationId::new("mismatch").unwrap(),
                ReceiptLimits::default()
            )
            .is_err()
        );
    }
    assert_eq!(bytes(&fixture.0), before);
}

#[cfg(target_os = "linux")]
#[test]
fn recorded_non_utf8_resources_round_trip_without_identity_loss() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let fixture = Fixture::new();
    let name = OsStr::from_bytes(b"resource-\xff");
    fs::write(fixture.source().join(name), b"native bytes").unwrap();
    let plan = fixture.plan(&["codex"]);
    let report = apply_plan(&plan, ApplyOptions::default());
    assert!(report.is_successful(), "{report:#?}");
    let original = receipt(&plan, &report, "non-unicode");
    let parsed = Receipt::from_json(
        &original.to_json(ReceiptLimits::default()).unwrap(),
        ReceiptLimits::default(),
    )
    .unwrap();
    let entry = parsed.document().sources[0]
        .entries
        .iter()
        .find(|entry| matches!(entry.path, NativePath::UnixBytes(_)))
        .unwrap();
    assert_eq!(
        entry.path.to_native().unwrap().as_os_str().as_bytes(),
        name.as_bytes()
    );
    assert_eq!(
        fs::read(fixture.destination().join(name)).unwrap(),
        b"native bytes"
    );
}

#[test]
fn unsupported_selection_is_recorded_while_the_whole_batch_stays_unattempted() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&["codex", "unknown-original-fixture"]);
    let before = bytes(&fixture.0);
    let report = apply_plan(&plan, ApplyOptions::default());
    let recorded = receipt(&plan, &report, "unsupported-selection");
    assert_eq!(
        recorded.document().preflight_failure.as_ref().unwrap().code,
        ReceiptFailureCode::InapplicablePlan
    );
    assert!(
        recorded
            .document()
            .destinations
            .iter()
            .all(|destination| destination.status == ReceiptStatus::NotAttempted)
    );
    let unsupported = recorded
        .document()
        .destinations
        .iter()
        .find(|destination| destination.planned_action == ReceiptPlanAction::Unsupported)
        .unwrap();
    assert!(unsupported.path.is_none());
    assert_eq!(
        unsupported.plan_diagnostics[0].code,
        ReceiptFailureCode::TargetResolution
    );
    assert_eq!(
        unsupported.plan_diagnostics[0].operation.as_deref(),
        Some("unknown target")
    );
    assert_eq!(bytes(&fixture.0), before);
}

#[cfg(target_os = "linux")]
#[test]
fn name_collision_receipt_preserves_both_native_path_labels() {
    let fixture = Fixture::new();
    let existing = fixture.0.join("home/.agents/skills/Sample");
    fs::create_dir_all(&existing).unwrap();
    fs::write(existing.join("sentinel"), b"unrelated").unwrap();
    let plan = fixture.plan(&["codex"]);
    let before = bytes(&fixture.0);
    let report = apply_plan(&plan, ApplyOptions::default());
    let recorded = receipt(&plan, &report, "case-collision");
    let conflicts = recorded.document().destinations[0]
        .plan_diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == ReceiptFailureCode::NameCollision)
        .collect::<Vec<_>>();
    assert_eq!(conflicts.len(), 2);
    assert_ne!(conflicts[0].path, conflicts[1].path);
    assert!(conflicts.iter().any(|diagnostic| {
        diagnostic
            .path
            .as_ref()
            .unwrap()
            .to_native()
            .unwrap()
            .ends_with("Sample")
    }));
    assert_eq!(bytes(&fixture.0), before);
}

#[test]
fn equivalent_sources_share_one_operation_with_every_receipt_reference() {
    let fixture = Fixture::new();
    let other = fixture.0.join("other-source/sample");
    fs::create_dir_all(other.join("empty")).unwrap();
    for name in ["SKILL.md", "resource"] {
        fs::copy(fixture.source().join(name), other.join(name)).unwrap();
    }
    let sources = [
        inventory_skill(&fixture.source(), InventoryLimits::default()).unwrap(),
        inventory_skill(&other, InventoryLimits::default()).unwrap(),
    ];
    let mut context = InstallContext::new(native_platform().unwrap());
    context.user_home = Some(fixture.0.join("home"));
    let plan = plan_install(
        &Catalog::bundled().unwrap(),
        InstallRequest {
            skills: &sources,
            target_ids: &["codex", "codex-cli"],
            scope: Scope::User,
            context: &context,
            limits: PlanLimits::default(),
        },
    )
    .unwrap();
    let report = apply_plan(&plan, ApplyOptions::default());
    assert!(report.is_successful(), "{report:#?}");
    let recorded = receipt(&plan, &report, "deduplicated-sources");
    assert_eq!(recorded.document().sources.len(), 2);
    assert_eq!(recorded.document().targets.len(), 1);
    assert_eq!(recorded.document().destinations.len(), 1);
    assert_eq!(
        recorded.document().destinations[0]
            .attributions
            .iter()
            .map(|reference| reference.source)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(
        Receipt::from_json(
            &recorded.to_json(ReceiptLimits::default()).unwrap(),
            ReceiptLimits::default()
        )
        .unwrap(),
        recorded
    );
}
