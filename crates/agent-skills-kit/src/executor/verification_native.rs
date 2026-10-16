use std::path::Path;

use super::*;
use crate::validation::{ValidatedSkill, inventory_retained, inventory_skill};

fn failure(path: Option<&Path>, kind: VerificationErrorKind) -> VerificationError {
    VerificationError {
        path: path.map(Path::to_owned),
        kind,
    }
}

struct Budget {
    limits: VerificationLimits,
    report: VerificationReport,
}
impl Budget {
    fn remaining(&self) -> Result<InventoryLimits, VerificationError> {
        let entries = self.limits.max_entries - self.report.entries;
        let bytes = self.limits.max_read_bytes - self.report.read_bytes;
        if entries == 0 {
            return Err(failure(None, VerificationErrorKind::Limit("entries")));
        }
        if bytes == 0 {
            return Err(failure(None, VerificationErrorKind::Limit("read bytes")));
        }
        let mut result = self.limits.inventory;
        result.max_entries = result.max_entries.min(entries);
        result.max_total_bytes = result.max_total_bytes.min(bytes);
        Ok(result)
    }
    fn record(&mut self, current: &ValidatedSkill) {
        self.report.entries += current.inventory().entries().len();
        self.report.read_bytes += current.inventory().total_bytes();
    }
}

fn same_snapshot(expected: &ValidatedSkill, current: &ValidatedSkill) -> bool {
    expected.inventory().root_stamp() == current.inventory().root_stamp()
        && expected.inventory().entries() == current.inventory().entries()
        && expected.metadata().source_bytes() == current.metadata().source_bytes()
}

fn namespaces(plan: &InstallPlan) -> Result<(), VerificationError> {
    for destination in plan.destinations() {
        // The public plan has no fabricated constructor; applicable operations
        // always retain a namespace observation.
        let snapshot = destination.namespace().expect("applicable namespace");
        if !snapshot
            .is_current()
            .map_err(|error| failure(destination.path(), VerificationErrorKind::Namespace(error)))?
        {
            return Err(failure(
                destination.path(),
                VerificationErrorKind::DestinationChanged,
            ));
        }
    }
    Ok(())
}

pub(super) fn verify(
    plan: &InstallPlan,
    limits: VerificationLimits,
) -> Result<VerificationReport, VerificationError> {
    if !limits.inventory.validate()
        || !(1..=1024 * 1024 * 1024).contains(&limits.max_read_bytes)
        || !(1..=65536).contains(&limits.max_entries)
    {
        return Err(failure(None, VerificationErrorKind::InvalidLimits));
    }
    if !plan.is_applicable() {
        return Err(failure(None, VerificationErrorKind::InapplicablePlan));
    }
    namespaces(plan)?;
    let mut budget = Budget {
        limits,
        report: VerificationReport {
            sources_checked: 0,
            destinations_checked: 0,
            read_bytes: 0,
            entries: 0,
        },
    };
    for expected in plan.sources() {
        let path = expected.inventory().requested_root();
        if !expected
            .inventory()
            .root_is_unchanged()
            .map_err(|error| failure(Some(path), VerificationErrorKind::SourceInspection(error)))?
        {
            return Err(failure(Some(path), VerificationErrorKind::SourceChanged));
        }
        let current = inventory_skill(path, budget.remaining()?)
            .map_err(|error| failure(Some(path), VerificationErrorKind::SourceInspection(error)))?;
        budget.record(&current);
        if !same_snapshot(expected, &current)
            || expected.inventory().canonical_root() != current.inventory().canonical_root()
        {
            return Err(failure(Some(path), VerificationErrorKind::SourceChanged));
        }
        budget.report.sources_checked += 1;
    }
    for destination in plan.destinations() {
        if let Some(expected) = destination.existing_payload() {
            let path = destination.path().expect("unchanged has a path");
            let current = inventory_retained(
                expected.inventory().retained_root().clone(),
                path,
                expected.metadata().name(),
                budget.remaining()?,
            )
            .map_err(|error| {
                failure(
                    Some(path),
                    VerificationErrorKind::DestinationInspection(error),
                )
            })?;
            budget.record(&current);
            if !same_snapshot(expected, &current) {
                return Err(failure(
                    Some(path),
                    VerificationErrorKind::DestinationChanged,
                ));
            }
        }
        budget.report.destinations_checked += 1;
    }
    namespaces(plan)?;
    Ok(budget.report)
}
