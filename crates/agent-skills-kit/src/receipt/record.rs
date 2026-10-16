use super::*;
use crate::{
    executor::{
        ApplyError, ApplyErrorKind, InstallReport, OutcomeStatus, VerificationError,
        VerificationErrorKind,
    },
    planner::{DestinationProblem, InstallPlan, NamespaceError, NamespaceErrorKind, PlannedAction},
    resolver::{ResolveError, native_platform},
    validation::{InventoryError, InventoryErrorKind, SourceEntryKind, TargetConstraintError},
};
use std::path::Path;

impl Receipt {
    /// Record one matching approved plan and its actual executor report as data.
    ///
    /// The report is borrowed, never consumed or hidden if receipt construction
    /// or later encoding fails. This does no filesystem I/O or revalidation.
    /// Callers must supply the plan/report from the same attempt. Shape, native
    /// labels, attribution and preview/outcome consistency are checked, but a
    /// mutable report or imported receipt is not authenticated by this method.
    /// Caller-selected operation IDs confer no ownership or cleanup authority.
    ///
    /// ```
    /// use agent_skills_kit::{executor::InstallReport, planner::InstallPlan,
    ///     receipt::{OperationId, Receipt, ReceiptError, ReceiptLimits}};
    /// fn encode_attempt(plan: &InstallPlan, report: &InstallReport)
    ///     -> Result<Vec<u8>, ReceiptError> {
    ///     let limits = ReceiptLimits::default();
    ///     Receipt::from_report(plan, report, OperationId::new("attempt-42")?, limits)?
    ///         .to_json(limits)
    /// }
    /// ```
    pub fn from_report(
        plan: &InstallPlan,
        report: &InstallReport,
        operation_id: OperationId,
        limits: ReceiptLimits,
    ) -> Result<Self, ReceiptError> {
        limits.validate()?;
        if report.outcomes.len() != plan.destinations().len() {
            return Err(ReceiptError::InvalidDocument(
                "report destination count differs from plan",
            ));
        }
        if !plan.is_applicable() && report.preflight_error.is_none() {
            return Err(ReceiptError::InvalidDocument(
                "blocked plan lacks preflight failure",
            ));
        }
        let platform = native_platform().ok_or(ReceiptError::UnsupportedPathPlatform)?;
        let mut builder = Builder {
            limits,
            path_units: 0,
            entries: 0,
            diagnostics: 0,
        };
        let mut sources = Vec::new();
        for source in plan.sources() {
            let root = builder.path(source.inventory().requested_root())?;
            let mut entries = Vec::new();
            for entry in source.inventory().entries() {
                builder.entries += 1;
                if builder.entries > limits.max_entries {
                    return Err(ReceiptError::Limit("payload entries"));
                }
                entries.push(ReceiptEntry {
                    path: builder.path(&entry.relative_path)?,
                    kind: match entry.kind {
                        SourceEntryKind::File => ReceiptEntryKind::File,
                        SourceEntryKind::Directory => ReceiptEntryKind::Directory,
                    },
                    mode: entry.output_mode,
                    bytes: if entry.kind == SourceEntryKind::File {
                        entry.stamp.length
                    } else {
                        0
                    },
                    digest: entry.digest.as_ref().map(ContentDigest::from_bytes),
                });
            }
            sources.push(ReceiptSource {
                root,
                name: source.metadata().name().to_owned(),
                root_mode: source.inventory().root_stamp().mode & 0o755,
                entries,
            });
        }
        let mut targets = Vec::new();
        for target in plan.targets() {
            if target.id().len() > 4096 {
                return Err(ReceiptError::InvalidDocument("target label length"));
            }
            targets.push(target.id().to_owned());
        }
        let mut destinations = Vec::new();
        for (planned, outcome) in plan.destinations().iter().zip(&report.outcomes) {
            if outcome.path.as_ref().map(|path| path.as_os_str())
                != planned.path().map(Path::as_os_str)
                || outcome.attributions.len() != planned.attributions().len()
            {
                return Err(ReceiptError::InvalidDocument(
                    "report destination differs from plan",
                ));
            }
            let mut attributions = Vec::new();
            for (expected, actual) in planned.attributions().iter().zip(&outcome.attributions) {
                if actual.target_id != plan.targets()[expected.target_index].id()
                    || actual.source.as_os_str()
                        != plan.sources()[expected.source_index]
                            .inventory()
                            .requested_root()
                            .as_os_str()
                {
                    return Err(ReceiptError::InvalidDocument(
                        "report attribution differs from plan",
                    ));
                }
                attributions.push(ReceiptAttribution {
                    source: expected.source_index,
                    target: expected.target_index,
                });
            }
            let mut plan_diagnostics = Vec::new();
            for problem in planned.problems() {
                builder.problem(problem, planned.path(), &mut plan_diagnostics)?;
            }
            let (status, failure) = match &outcome.status {
                OutcomeStatus::Created => (ReceiptStatus::Created, None),
                OutcomeStatus::Unchanged => (ReceiptStatus::Unchanged, None),
                OutcomeStatus::Failed(error) => {
                    (ReceiptStatus::Failed, Some(builder.apply(error)?))
                }
                OutcomeStatus::CommittedWithPermissionFailure(error) => {
                    if planned.action() != PlannedAction::Create {
                        return Err(ReceiptError::InvalidDocument(
                            "committed permission failure without create preview",
                        ));
                    }
                    let mut diagnostic = builder.apply(error)?;
                    // Preserve schema-1's coarse cause/path/OS code, and make the
                    // committed effect explicit without adding incompatible fields.
                    // The live report retains the original typed operation/cause.
                    diagnostic.operation = Some(
                        "payload committed; destination permission finalization failed".to_owned(),
                    );
                    (ReceiptStatus::Failed, Some(diagnostic))
                }
                OutcomeStatus::NotAttempted => (ReceiptStatus::NotAttempted, None),
            };
            destinations.push(ReceiptDestination {
                path: planned.path().map(|path| builder.path(path)).transpose()?,
                attributions,
                planned_action: match planned.action() {
                    PlannedAction::Create => ReceiptPlanAction::Create,
                    PlannedAction::Unchanged => ReceiptPlanAction::Unchanged,
                    PlannedAction::Conflict => ReceiptPlanAction::Conflict,
                    PlannedAction::Unsupported => ReceiptPlanAction::Unsupported,
                },
                plan_diagnostics,
                status,
                failure,
            });
        }
        let preflight_failure = report
            .preflight_error
            .as_ref()
            .map(|error| builder.verification(error))
            .transpose()?;
        let mut cleanup_diagnostics = Vec::new();
        for error in &report.cleanup_errors {
            cleanup_diagnostics.push(builder.apply(error)?);
        }
        Self::new(
            ReceiptDocument {
                schema_version: RECEIPT_SCHEMA_VERSION,
                operation_id,
                platform,
                scope: plan.scope(),
                digest_algorithm: DigestAlgorithm::Blake3,
                sources,
                targets,
                destinations,
                preflight_failure,
                cleanup_diagnostics,
            },
            limits,
        )
    }
}

struct Builder {
    limits: ReceiptLimits,
    path_units: usize,
    entries: usize,
    diagnostics: usize,
}
impl Builder {
    fn path(&mut self, path: &Path) -> Result<NativePath, ReceiptError> {
        let encoded = NativePath::from_native(path)?;
        encoded.validate(self.limits.max_path_units)?;
        self.path_units = self
            .path_units
            .checked_add(encoded.units())
            .ok_or(ReceiptError::Limit("path units"))?;
        if self.path_units > self.limits.max_total_path_units {
            return Err(ReceiptError::Limit("path units"));
        }
        Ok(encoded)
    }
    fn diagnostic(
        &mut self,
        code: ReceiptFailureCode,
        path: Option<&Path>,
        operation: Option<&str>,
        os_error: Option<i32>,
    ) -> Result<ReceiptDiagnostic, ReceiptError> {
        self.diagnostics += 1;
        if self.diagnostics > self.limits.max_diagnostics {
            return Err(ReceiptError::Limit("diagnostics"));
        }
        if operation.is_some_and(|operation| operation.len() > 256) {
            return Err(ReceiptError::InvalidDocument("diagnostic operation length"));
        }
        Ok(ReceiptDiagnostic {
            code,
            path: path.map(|path| self.path(path)).transpose()?,
            operation: operation.map(str::to_owned),
            os_error,
        })
    }
    fn inventory(
        &mut self,
        code: ReceiptFailureCode,
        error: &InventoryError,
    ) -> Result<ReceiptDiagnostic, ReceiptError> {
        let (operation, number) = match &error.kind {
            InventoryErrorKind::Io {
                operation,
                raw_os_error,
                ..
            } => (Some(*operation), *raw_os_error),
            InventoryErrorKind::Limit(label) => (Some(*label), None),
            _ => (None, None),
        };
        self.diagnostic(code, Some(&error.path), operation, number)
    }
    fn namespace(&mut self, error: &NamespaceError) -> Result<ReceiptDiagnostic, ReceiptError> {
        let (operation, number) = match &error.kind {
            NamespaceErrorKind::Io {
                operation,
                raw_os_error,
                ..
            } => (Some(*operation), *raw_os_error),
            NamespaceErrorKind::Limit(label) => (Some(*label), None),
            _ => (None, None),
        };
        self.diagnostic(
            ReceiptFailureCode::Namespace,
            Some(&error.path),
            operation,
            number,
        )
    }
    fn verification(
        &mut self,
        error: &VerificationError,
    ) -> Result<ReceiptDiagnostic, ReceiptError> {
        use ReceiptFailureCode as Code;
        let code = match &error.kind {
            VerificationErrorKind::UnsupportedPlatform => Code::UnsupportedPlatform,
            VerificationErrorKind::InapplicablePlan => Code::InapplicablePlan,
            VerificationErrorKind::InvalidLimits => Code::InvalidLimits,
            VerificationErrorKind::Limit(label) => {
                return self.diagnostic(Code::Limit, error.path.as_deref(), Some(label), None);
            }
            VerificationErrorKind::SourceChanged => Code::SourceChanged,
            VerificationErrorKind::SourceInspection(error) => {
                return self.inventory(Code::SourceInspection, error);
            }
            VerificationErrorKind::Namespace(error) => return self.namespace(error),
            VerificationErrorKind::DestinationChanged => Code::DestinationChanged,
            VerificationErrorKind::DestinationInspection(error) => {
                return self.inventory(Code::DestinationInspection, error);
            }
        };
        self.diagnostic(code, error.path.as_deref(), None, None)
    }
    fn apply(&mut self, error: &ApplyError) -> Result<ReceiptDiagnostic, ReceiptError> {
        use ReceiptFailureCode as Code;
        let code = match &error.kind {
            ApplyErrorKind::SourceChanged => Code::SourceChanged,
            ApplyErrorKind::SourceInspection(error) => {
                return self.inventory(Code::SourceInspection, error);
            }
            ApplyErrorKind::DestinationChanged => Code::DestinationChanged,
            ApplyErrorKind::Namespace(error) => return self.namespace(error),
            ApplyErrorKind::DestinationInspection(error) => {
                return self.inventory(Code::DestinationInspection, error);
            }
            ApplyErrorKind::StageChanged => Code::StageChanged,
            ApplyErrorKind::CreatedParentRetained => Code::CreatedParentRetained,
            ApplyErrorKind::Unsupported(label) => {
                return self.diagnostic(Code::Unsupported, Some(&error.path), Some(label), None);
            }
            ApplyErrorKind::Io {
                operation,
                raw_os_error,
                ..
            } => {
                return self.diagnostic(
                    Code::Io,
                    Some(&error.path),
                    Some(operation),
                    *raw_os_error,
                );
            }
        };
        self.diagnostic(code, Some(&error.path), None, None)
    }
    fn problem(
        &mut self,
        problem: &DestinationProblem,
        path: Option<&Path>,
        output: &mut Vec<ReceiptDiagnostic>,
    ) -> Result<(), ReceiptError> {
        use ReceiptFailureCode as Code;
        let (code, label) = match problem {
            DestinationProblem::Target(error) => {
                (Code::TargetResolution, Some(resolution_label(error)))
            }
            DestinationProblem::Host(TargetConstraintError::ReservedName(_)) => {
                (Code::HostConstraint, Some("reserved skill name"))
            }
            DestinationProblem::Host(TargetConstraintError::AsciiNameRequired) => {
                (Code::HostConstraint, Some("ASCII skill name required"))
            }
            DestinationProblem::Namespace(error) => {
                output.push(self.namespace(error)?);
                return Ok(());
            }
            DestinationProblem::ExistingInspection(error) => {
                output.push(self.inventory(Code::DestinationInspection, error)?);
                return Ok(());
            }
            DestinationProblem::ExistingContentDiffers => (Code::ExistingContentDiffers, None),
            DestinationProblem::ExistingNameMismatch => (Code::ExistingNameMismatch, None),
            DestinationProblem::CompetingSources => (Code::CompetingSources, None),
            DestinationProblem::PortableNameCollision { first, second } => {
                output.push(self.diagnostic(Code::NameCollision, Some(first), None, None)?);
                output.push(self.diagnostic(Code::NameCollision, Some(second), None, None)?);
                return Ok(());
            }
        };
        output.push(self.diagnostic(code, path, label, None)?);
        Ok(())
    }
}
fn resolution_label(error: &ResolveError) -> &'static str {
    match error {
        ResolveError::UnknownTarget(_) => "unknown target",
        ResolveError::UnsupportedScope { .. } => "unsupported scope",
        ResolveError::ForeignPlatform { .. } => "foreign platform",
        ResolveError::UnsupportedPlatform(_) => "unsupported platform",
        ResolveError::UnsupportedStatus(_) => "unsupported catalog status",
        ResolveError::ConfigurationRequired => "configuration required",
        ResolveError::NoWriteLocation => "no write location",
        ResolveError::MissingContext(_) => "missing explicit context",
        ResolveError::MissingOverride(_) => "missing documented override",
        ResolveError::InvalidContext { .. } => "invalid explicit context",
        ResolveError::InvalidOverride { .. } => "invalid documented override",
        ResolveError::InvalidRuleComponent(_) => "invalid rule component",
        ResolveError::UnreviewedRule => "unreviewed rule",
    }
}
