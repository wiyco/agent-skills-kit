//! Standalone consumer of the public library; all writes use an owned temporary root.
#![forbid(unsafe_code)]

use agent_skills_kit::{
    catalog::{Catalog, Scope},
    executor::{ApplyOptions, OutcomeStatus, apply_plan},
    planner::{InstallRequest, PlanLimits, PlannedAction, plan_install},
    receipt::{OperationId, Receipt, ReceiptLimits},
    resolver::{InstallContext, native_platform, resolve},
    validation::{InventoryLimits, inventory_skill},
};
use std::{
    error::Error,
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
struct Fixture {
    root: PathBuf,
    source: PathBuf,
    context: InstallContext,
}
impl Fixture {
    fn new() -> Result<Self> {
        let platform =
            native_platform().ok_or_else(|| io::Error::other("unsupported native platform"))?;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let mut root = None;
        for _ in 0..100 {
            let candidate = std::env::temp_dir().join(format!(
                "ask-external-consumer-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => {
                    root = Some(candidate);
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        let root = root.ok_or_else(|| io::Error::other("temporary fixture name budget"))?;
        let source = root.join("source/example-skill");
        let mut context = InstallContext::new(platform);
        context.user_home = Some(root.join("isolated home"));
        context.project_root = Some(root.join("isolated project"));
        // Establish teardown ownership before any remaining fallible setup.
        let fixture = Self {
            root,
            source,
            context,
        };
        fs::create_dir_all(fixture.source.join("empty directory"))?;
        fs::create_dir(fixture.context.user_home.as_ref().unwrap())?;
        fs::create_dir(fixture.context.project_root.as_ref().unwrap())?;
        fs::write(
            fixture.source.join("SKILL.md"),
            b"---\nname: example-skill\ndescription: Original inert consumer fixture\n---\nData only.\n",
        )?;
        fs::write(fixture.source.join("reference café.txt"), b"original bytes")?;
        Ok(fixture)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove only the owned consumer fixture");
    }
}

fn demonstrate(scope: Scope) -> Result<Receipt> {
    let fixture = Fixture::new()?;
    let catalog = Catalog::bundled()?;
    let targets = ["codex", "codex-cli", "claude-code", "opencode"];
    // Resolution and validation are reusable independently of planning/apply.
    for target in targets {
        resolve(&catalog, target, scope, &fixture.context)?;
    }
    let sources = [inventory_skill(
        &fixture.source,
        InventoryLimits::default(),
    )?];
    let preview = || {
        plan_install(
            &catalog,
            InstallRequest {
                skills: &sources,
                target_ids: &targets,
                scope,
                context: &fixture.context,
                limits: PlanLimits::default(),
            },
        )
    };
    let plan = preview()?;
    assert!(plan.is_applicable());
    assert_eq!(plan.destinations().len(), 3, "alias must deduplicate");
    assert!(
        plan.destinations()
            .iter()
            .all(|row| row.action() == PlannedAction::Create)
    );
    assert!(
        plan.destinations()
            .iter()
            .all(|row| !row.path().unwrap().exists())
    );
    // This demonstration explicitly approves the plan for its owned fixture.
    // A real application presents/reviews the plan before this call.
    let report = apply_plan(&plan, ApplyOptions::default());
    assert!(report.is_successful(), "{report:?}");
    for outcome in &report.outcomes {
        assert_eq!(outcome.status, OutcomeStatus::Created);
        let path = outcome.path.as_ref().unwrap();
        assert_eq!(
            fs::read(path.join("SKILL.md"))?,
            fs::read(fixture.source.join("SKILL.md"))?
        );
        assert_eq!(
            fs::read(path.join("reference café.txt"))?,
            b"original bytes"
        );
        assert!(path.join("empty directory").is_dir());
    }
    let receipt = Receipt::from_report(
        &plan,
        &report,
        OperationId::new("consumer-example")?,
        ReceiptLimits::default(),
    )?;
    let encoded = receipt.to_json(ReceiptLimits::default())?;
    assert_eq!(
        Receipt::from_json(&encoded, ReceiptLimits::default())?,
        receipt
    );
    let unchanged = apply_plan(&preview()?, ApplyOptions::default());
    assert!(unchanged.is_successful(), "{unchanged:?}");
    assert!(
        unchanged
            .outcomes
            .iter()
            .all(|row| row.status == OutcomeStatus::Unchanged)
    );
    let edited = plan.destinations()[0]
        .path()
        .unwrap()
        .join("reference café.txt");
    fs::write(&edited, b"manual edit")?;
    let conflict = preview()?;
    assert!(!conflict.is_applicable());
    let refused = apply_plan(&conflict, ApplyOptions::default());
    assert!(refused.preflight_error.is_some());
    assert!(
        refused
            .outcomes
            .iter()
            .all(|row| row.status == OutcomeStatus::NotAttempted)
    );
    assert_eq!(fs::read(&edited)?, b"manual edit");
    Ok(receipt)
}
fn main() -> Result<()> {
    demonstrate(Scope::Project)?;
    println!(
        "Public API preview, copy, receipt and unchanged checks passed in an owned temporary root."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_skills_kit::{receipt::ReceiptError, resolver::ResolveError};

    #[test]
    fn core_targets_work_without_the_cli_in_both_scopes() -> Result<()> {
        demonstrate(Scope::User)?;
        demonstrate(Scope::Project)?;
        Ok(())
    }

    #[test]
    fn unknown_targets_unsupported_scopes_and_future_receipts_are_typed() -> Result<()> {
        let fixture = Fixture::new()?;
        let catalog = Catalog::bundled()?;
        assert!(matches!(
            resolve(&catalog, "unknown", Scope::User, &fixture.context),
            Err(ResolveError::UnknownTarget(_))
        ));
        assert!(matches!(
            resolve(
                &catalog,
                "codex",
                Scope::ApplicationWorkspace,
                &fixture.context
            ),
            Err(ResolveError::UnsupportedScope { .. })
        ));
        assert!(matches!(
            Receipt::from_json(br#"{"schema_version":999}"#, ReceiptLimits::default()),
            Err(ReceiptError::UnsupportedVersion(999))
        ));
        assert!(
            !fixture
                .context
                .user_home
                .as_ref()
                .unwrap()
                .join(".agents")
                .exists()
        );
        Ok(())
    }

    #[test]
    fn custom_registry_entries_need_no_library_fork_or_cli() -> Result<()> {
        let fixture = Fixture::new()?;
        let bundled = Catalog::bundled()?;
        let mut document = bundled.document();
        // Original test-only registration reuses the documented Codex placement.
        // It is not a newly verified product/host variant or a new path rule.
        let mut custom = bundled.get("codex").unwrap().clone();
        custom.id = "consumer-codex-placement".into();
        custom.aliases = vec!["consumer-alias".into()];
        custom.variant = "synthetic-consumer-registration".into();
        document.targets.push(custom);
        let catalog = Catalog::new(document)?;
        let original = resolve(&catalog, "codex", Scope::Project, &fixture.context)?;
        let selected = resolve(&catalog, "consumer-alias", Scope::Project, &fixture.context)?;
        assert_eq!(selected.preferred_write.path, original.preferred_write.path);
        let sources = [inventory_skill(
            &fixture.source,
            InventoryLimits::default(),
        )?];
        let plan = plan_install(
            &catalog,
            InstallRequest {
                skills: &sources,
                target_ids: &["consumer-alias"],
                scope: Scope::Project,
                context: &fixture.context,
                limits: PlanLimits::default(),
            },
        )?;
        let report = apply_plan(&plan, ApplyOptions::default());
        assert!(report.is_successful(), "{report:?}");
        assert_eq!(
            report.outcomes[0].attributions[0].target_id,
            "consumer-codex-placement"
        );
        Ok(())
    }
}
