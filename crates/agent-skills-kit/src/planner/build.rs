use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
};

use super::*;
use crate::planner::{
    DestinationPresence, NamespaceErrorKind,
    names::{ComponentKey, component_key, reject_payload_collisions},
    namespace::DirectoryAnchor,
    observe_destination,
};
use crate::{
    catalog::ContextBase,
    resolver::{PathOrigin, add_evidence_warnings, resolve},
    validation::{SourceEntryKind, inventory_retained, inventory_skill},
};

use crate::planner::native;
use native::{Directory, Identity};
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum DestinationKey {
    Existing(Identity),
    Missing(Identity, Vec<OsString>),
    Unresolved(usize, usize),
}
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum WriteKey {
    Parent(Identity, Vec<OsString>),
    Payload(usize, PathBuf),
}

struct Budget {
    limits: PlanLimits,
    bytes: u64,
    entries: usize,
    paths: usize,
    intended: usize,
}
impl Budget {
    fn new(limits: PlanLimits) -> Self {
        Self {
            limits,
            bytes: 0,
            entries: 0,
            paths: 0,
            intended: 0,
        }
    }
    fn path(&mut self, path: &Path) -> Result<(), PlanError> {
        self.paths = self
            .paths
            .checked_add(native::path_bytes(path))
            .ok_or(PlanError::Limit("path bytes"))?;
        if self.paths > self.limits.max_path_bytes {
            Err(PlanError::Limit("path bytes"))
        } else {
            Ok(())
        }
    }
    fn entry(&mut self) -> Result<(), PlanError> {
        self.entries += 1;
        if self.entries > self.limits.max_inspected_entries {
            Err(PlanError::Limit("inspected entries"))
        } else {
            Ok(())
        }
    }
    fn write(&mut self, path: &Path) -> Result<(), PlanError> {
        self.intended += 1;
        if self.intended > self.limits.max_intended_entries {
            return Err(PlanError::Limit("intended entries"));
        }
        self.path(path)
    }
    fn inventory_limits(&self) -> Result<InventoryLimits, PlanError> {
        let mut limits = self.limits.inventory;
        let entries = self.limits.max_inspected_entries - self.entries;
        let bytes = self.limits.max_read_bytes - self.bytes;
        if entries == 0 {
            return Err(PlanError::Limit("inspected entries"));
        }
        if bytes == 0 {
            return Err(PlanError::Limit("read bytes"));
        }
        limits.max_entries = limits.max_entries.min(entries);
        limits.max_total_bytes = limits.max_total_bytes.min(bytes);
        Ok(limits)
    }
}

pub(super) fn plan(
    catalog: &Catalog,
    request: InstallRequest<'_>,
) -> Result<InstallPlan, PlanError> {
    if !request.limits.valid() {
        return Err(PlanError::InvalidLimits);
    }
    if request.skills.is_empty() {
        return Err(PlanError::EmptySelection("sources"));
    }
    if request.target_ids.is_empty() {
        return Err(PlanError::EmptySelection("targets"));
    }
    if request.skills.len() > request.limits.max_skills {
        return Err(PlanError::Limit("sources"));
    }
    if request.target_ids.len() > request.limits.max_targets {
        return Err(PlanError::Limit("targets"));
    }
    let mut budget = Budget::new(request.limits);
    let sources = prepare_sources(request.skills, &mut budget)?;
    let targets = prepare_targets(catalog, &request);
    if sources
        .len()
        .checked_mul(targets.len())
        .is_none_or(|count| count > request.limits.max_destinations)
    {
        return Err(PlanError::Limit("destinations"));
    }
    let mut source_ancestors = Vec::new();
    for source in &sources {
        source_ancestors.push(ancestors(
            source.inventory().retained_root(),
            source.inventory().canonical_root(),
        )?);
    }
    for first in 0..sources.len() {
        for second in first + 1..sources.len() {
            if source_ancestors[first].contains(&source_id(&sources[second]))
                || source_ancestors[second].contains(&source_id(&sources[first]))
            {
                return Err(PlanError::Overlap {
                    first: sources[first].inventory().canonical_root().to_owned(),
                    second: sources[second].inventory().canonical_root().to_owned(),
                });
            }
        }
    }
    let mut groups: BTreeMap<DestinationKey, PlannedDestination> = BTreeMap::new();
    for (source_index, source) in sources.iter().enumerate() {
        for (target_index, target) in targets.iter().enumerate() {
            let attribution = TargetAttribution {
                source_index,
                target_index,
            };
            let mut destination = candidate(
                catalog,
                source,
                target,
                attribution,
                request.context,
                &mut budget,
            )?;
            let key = destination_key(&destination, source_index, target_index);
            if let Some(existing) = groups.get_mut(&key) {
                if !same_payload(&sources[existing.source_index], source) {
                    existing.action = PlannedAction::Conflict;
                    if !existing
                        .problems
                        .contains(&DestinationProblem::CompetingSources)
                    {
                        existing.problems.push(DestinationProblem::CompetingSources);
                    }
                }
                if matches!(
                    destination.action,
                    PlannedAction::Conflict | PlannedAction::Unsupported
                ) {
                    existing.action = destination.action;
                }
                for problem in &destination.problems {
                    if !existing.problems.contains(problem) {
                        existing.problems.push(problem.clone());
                    }
                }
                existing.attributions.push(attribution);
                if destination.path < existing.path {
                    existing.path = destination.path.take();
                    existing.namespace = destination.namespace.take();
                }
            } else {
                groups.insert(key, destination);
            }
        }
    }
    let mut destinations = groups.into_values().collect::<Vec<_>>();
    destinations.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.attributions.cmp(&b.attributions))
    });
    for destination in &mut destinations {
        destination.attributions.sort();
        destination.attributions.dedup();
    }
    reject_overlaps(&sources, &source_ancestors, &destinations)?;
    reject_missing_prefix_collisions(&destinations, &mut budget)?;
    let writes = intended_writes(&sources, &mut destinations, &mut budget)?;
    for destination in &destinations {
        if let Some(snapshot) = &destination.namespace {
            if !snapshot.is_current().map_err(PlanError::Namespace)? {
                return Err(PlanError::Namespace(NamespaceError::new(
                    snapshot.requested_path(),
                    NamespaceErrorKind::NamespaceChanged,
                )));
            }
        }
    }
    Ok(InstallPlan {
        scope: request.scope,
        sources,
        targets,
        destinations,
        inspected_bytes: budget.bytes,
        writes,
    })
}

fn prepare_sources(
    input: &[ValidatedSkill],
    budget: &mut Budget,
) -> Result<Vec<ValidatedSkill>, PlanError> {
    let mut selected = input.iter().collect::<Vec<_>>();
    selected.sort_by(|a, b| {
        a.inventory()
            .canonical_root()
            .cmp(b.inventory().canonical_root())
            .then(
                a.inventory()
                    .requested_root()
                    .cmp(b.inventory().requested_root()),
            )
    });
    let mut sources = Vec::new();
    let mut seen = BTreeSet::new();
    for source in selected {
        if !source
            .inventory()
            .root_is_unchanged()
            .map_err(PlanError::SourceInspection)?
        {
            return Err(PlanError::StaleSource(
                source.inventory().requested_root().to_owned(),
            ));
        }
        let current = inventory_skill(
            source.inventory().requested_root(),
            budget.inventory_limits()?,
        )
        .map_err(PlanError::SourceInspection)?;
        budget.bytes += current.inventory().total_bytes();
        for entry in current.inventory().entries() {
            budget.entry()?;
            budget.path(&entry.relative_path)?;
        }
        budget.path(current.inventory().canonical_root())?;
        if source.inventory().root_stamp() != current.inventory().root_stamp()
            || source.inventory().entries() != current.inventory().entries()
            || source.inventory().canonical_root() != current.inventory().canonical_root()
            || source.metadata().source_bytes() != current.metadata().source_bytes()
        {
            return Err(PlanError::StaleSource(
                source.inventory().requested_root().to_owned(),
            ));
        }
        reject_payload_collisions(current.inventory().entries())?;
        if seen.insert(source_id(&current)) {
            sources.push(current);
        }
    }
    Ok(sources)
}

fn prepare_targets(catalog: &Catalog, request: &InstallRequest<'_>) -> Vec<PlannedTarget> {
    let mut targets = BTreeMap::new();
    for raw in request.target_ids {
        let definition = catalog.get(raw);
        let id = definition.map_or_else(|| (*raw).to_owned(), |target| target.id.clone());
        if targets.contains_key(&id) {
            continue;
        }
        let resolution = resolve(catalog, &id, request.scope, request.context);
        let mut evidence = Vec::new();
        let mut warnings = BTreeSet::from([ResolutionWarning::PlacementNotExclusive]);
        if let Some(definition) = definition {
            for limitation in &definition.limitations {
                warnings.insert(ResolutionWarning::DocumentedLimitation(limitation.clone()));
            }
            if let Some(scope) = definition
                .scopes
                .iter()
                .find(|scope| scope.scope == request.scope)
            {
                for limitation in &scope.limitations {
                    warnings.insert(ResolutionWarning::DocumentedLimitation(limitation.clone()));
                }
                for rule in scope
                    .preferred_write
                    .iter()
                    .chain(scope.discovery.iter().map(|read| &read.location))
                {
                    add_evidence_warnings(&rule.evidence, &mut warnings);
                    evidence.push(rule.evidence.clone());
                }
            }
        }
        if let Ok(resolved) = &resolution {
            warnings.extend(resolved.warnings.iter().cloned());
        }
        targets.insert(
            id.clone(),
            PlannedTarget {
                id,
                resolution,
                evidence,
                warnings: warnings.into_iter().collect(),
            },
        );
    }
    targets.into_values().collect()
}

fn candidate(
    catalog: &Catalog,
    source: &ValidatedSkill,
    target: &PlannedTarget,
    attribution: TargetAttribution,
    context: &InstallContext,
    budget: &mut Budget,
) -> Result<PlannedDestination, PlanError> {
    let mut result = PlannedDestination {
        path: None,
        action: PlannedAction::Unsupported,
        problems: Vec::new(),
        attributions: vec![attribution],
        source_index: attribution.source_index,
        namespace: None,
        writes: Vec::new(),
        existing: None,
    };
    let resolved = match &target.resolution {
        Ok(resolved) => resolved,
        Err(error) => {
            result
                .problems
                .push(DestinationProblem::Target(error.clone()));
            return Ok(result);
        }
    };
    result.path = Some(resolved.preferred_write.path.join(source.metadata().name()));
    let definition = catalog
        .get(&target.id)
        .expect("successful resolution has a catalog definition");
    if let Err(error) = source.metadata().validate_for_target(definition) {
        result.problems.push(DestinationProblem::Host(error));
        return Ok(result);
    }
    let root = match &resolved.preferred_write.origin {
        PathOrigin::Context(base) | PathOrigin::Fallback { base, .. } => {
            context_path(*base, context)
        }
        PathOrigin::Override(name) => context.overrides.get(name).map(PathBuf::from),
    }
    .ok_or_else(|| {
        PlanError::Namespace(NamespaceError::new(
            &resolved.preferred_write.path,
            NamespaceErrorKind::InvalidRoot,
        ))
    })?;
    let relative = resolved
        .preferred_write
        .path
        .strip_prefix(&root)
        .map_err(|_| {
            PlanError::Namespace(NamespaceError::new(
                &resolved.preferred_write.path,
                NamespaceErrorKind::InvalidRelativePath,
            ))
        })?
        .join(source.metadata().name());
    match observe_destination(&root, &relative) {
        Ok(snapshot) => {
            budget.path(snapshot.destination())?;
            for (path, _) in snapshot.existing_directories() {
                budget.path(path)?;
            }
            result.path = Some(snapshot.destination().to_owned());
            if snapshot.presence() == DestinationPresence::Missing {
                if let Some((first, second)) = existing_name_collision(&snapshot, budget)? {
                    result.action = PlannedAction::Conflict;
                    result
                        .problems
                        .push(DestinationProblem::PortableNameCollision { first, second });
                } else {
                    result.action = PlannedAction::Create;
                }
            } else {
                compare_existing(&snapshot, source, &mut result, budget)?;
            }
            result.namespace = Some(snapshot);
        }
        Err(error) => {
            result.action = if matches!(
                error.kind,
                NamespaceErrorKind::UnsupportedPlatform
                    | NamespaceErrorKind::UnsupportedFilesystem(_)
            ) {
                PlannedAction::Unsupported
            } else {
                PlannedAction::Conflict
            };
            result.problems.push(DestinationProblem::Namespace(error));
        }
    }
    Ok(result)
}
fn compare_existing(
    snapshot: &DestinationSnapshot,
    source: &ValidatedSkill,
    result: &mut PlannedDestination,
    budget: &mut Budget,
) -> Result<(), PlanError> {
    result.action = PlannedAction::Conflict;
    let leaf = snapshot.anchors.last().expect("existing root retained");
    // A case-insensitive native lookup can succeed using a different raw name.
    // Verify actual spelling before treating metadata as matching that directory.
    let parent = snapshot
        .anchors
        .iter()
        .rev()
        .nth(1)
        .expect("skill has a parent");
    let wanted = std::ffi::OsStr::new(source.metadata().name());
    let mut exact_name = false;
    for entry in directory_names(parent, budget, "enumerate existing skill spelling")? {
        let name = entry.map_err(|error| {
            PlanError::Namespace(NamespaceError::io(
                &parent.path,
                "read existing skill spelling",
                error,
            ))
        })?;
        budget.entry()?;
        if name == wanted {
            exact_name = true;
        }
    }
    if !exact_name {
        result
            .problems
            .push(DestinationProblem::ExistingNameMismatch);
        return Ok(());
    }
    let limits = budget.inventory_limits()?;
    let existing = match inventory_retained(
        Arc::clone(&leaf.handle),
        &leaf.path,
        source.metadata().name(),
        limits,
    ) {
        Ok(existing) => existing,
        Err(error) => {
            // A failed bounded traversal may already have read up to its entire
            // allowance. Reserve that allowance rather than undercount reads
            // when subsequent destinations are inspected.
            budget.bytes += limits.max_total_bytes;
            budget.entries += limits.max_entries;
            result
                .problems
                .push(DestinationProblem::ExistingInspection(error));
            return Ok(());
        }
    };
    budget.bytes += existing.inventory().total_bytes();
    for entry in existing.inventory().entries() {
        budget.entry()?;
        budget.path(&entry.relative_path)?;
    }
    let identical = existing.metadata().source_bytes() == source.metadata().source_bytes()
        && existing.inventory().root_stamp().mode & 0o7777
            == source.inventory().root_stamp().mode & 0o755
        && existing.inventory().entries().len() == source.inventory().entries().len()
        && existing
            .inventory()
            .entries()
            .iter()
            .zip(source.inventory().entries())
            .all(|(actual, intended)| {
                actual.relative_path == intended.relative_path
                    && actual.kind == intended.kind
                    && actual.stamp.mode & 0o7777 == intended.output_mode
                    && actual.digest == intended.digest
                    && (actual.kind != SourceEntryKind::File
                        || actual.stamp.length == intended.stamp.length)
            });
    if identical {
        result.action = PlannedAction::Unchanged;
        result.existing = Some(existing);
    } else {
        result
            .problems
            .push(DestinationProblem::ExistingContentDiffers);
    }
    Ok(())
}

fn context_path(base: ContextBase, context: &InstallContext) -> Option<PathBuf> {
    match base {
        ContextBase::Home => context.user_home.clone(),
        ContextBase::Project => context.project_root.clone(),
        ContextBase::ApplicationWorkspace => context.application_workspace.clone(),
    }
}
fn source_id(source: &ValidatedSkill) -> Identity {
    source.inventory().root_stamp().file_identity()
}
fn anchor_id(anchor: &DirectoryAnchor) -> Identity {
    anchor.stamp.file_identity()
}
fn destination_key(
    destination: &PlannedDestination,
    source: usize,
    target: usize,
) -> DestinationKey {
    match &destination.namespace {
        None => DestinationKey::Unresolved(source, target),
        Some(snapshot) => {
            let anchor = snapshot.anchors.last().expect("observations retain a root");
            if snapshot.missing.is_empty() {
                DestinationKey::Existing(anchor_id(anchor))
            } else {
                DestinationKey::Missing(anchor_id(anchor), snapshot.missing.clone())
            }
        }
    }
}
fn same_payload(a: &ValidatedSkill, b: &ValidatedSkill) -> bool {
    a.inventory().root_stamp().mode & 0o755 == b.inventory().root_stamp().mode & 0o755
        && a.inventory().entries().len() == b.inventory().entries().len()
        && a.inventory()
            .entries()
            .iter()
            .zip(b.inventory().entries())
            .all(|(a, b)| {
                a.relative_path == b.relative_path
                    && a.kind == b.kind
                    && a.output_mode == b.output_mode
                    && a.digest == b.digest
                    && (a.kind != SourceEntryKind::File || a.stamp.length == b.stamp.length)
            })
}

fn ancestors(start: &Arc<Directory>, label: &Path) -> Result<BTreeSet<Identity>, PlanError> {
    let mut result = BTreeSet::new();
    let mut current = Arc::clone(start);
    for _ in 0..256 {
        let identity = native::identity(&current).map_err(|error| {
            PlanError::Namespace(NamespaceError::io(
                label,
                "inspect native ancestor identity",
                error,
            ))
        })?;
        if !result.insert(identity) {
            return Err(PlanError::Namespace(NamespaceError::new(
                label,
                NamespaceErrorKind::NamespaceChanged,
            )));
        }
        let parent = native::parent(&current).map_err(|error| {
            PlanError::Namespace(NamespaceError::io(label, "open native ancestor", error))
        })?;
        if native::identity(&parent).map_err(|error| {
            PlanError::Namespace(NamespaceError::io(label, "inspect native parent", error))
        })? == identity
        {
            return Ok(result);
        }
        current = parent;
    }
    Err(PlanError::Namespace(NamespaceError::new(
        label,
        NamespaceErrorKind::Limit("ancestor depth"),
    )))
}

fn reject_overlaps(
    sources: &[ValidatedSkill],
    source_ancestors: &[BTreeSet<Identity>],
    destinations: &[PlannedDestination],
) -> Result<(), PlanError> {
    let mut ancestry = Vec::new();
    for destination in destinations {
        let Some(snapshot) = &destination.namespace else {
            ancestry.push(None);
            continue;
        };
        let anchor = snapshot.anchors.last().expect("retained root");
        let parents = ancestors(&anchor.handle, &anchor.path)?;
        for (index, source) in sources.iter().enumerate() {
            // An already validated identical payload at the exact source root
            // has no intended writes. Nested/containing destinations still fail.
            if snapshot.missing.is_empty()
                && anchor_id(anchor) == source_id(source)
                && destination.action == PlannedAction::Unchanged
                && destination.source_index() == index
            {
                continue;
            }
            if parents.contains(&source_id(source))
                || (snapshot.missing.is_empty()
                    && source_ancestors[index].contains(&anchor_id(anchor)))
            {
                return Err(PlanError::Overlap {
                    first: source.inventory().canonical_root().to_owned(),
                    second: snapshot.destination().to_owned(),
                });
            }
        }
        ancestry.push(Some(parents));
    }
    for first in 0..destinations.len() {
        let Some(a) = &destinations[first].namespace else {
            continue;
        };
        let aa = a.anchors.last().expect("retained root");
        for second in first + 1..destinations.len() {
            let Some(b) = &destinations[second].namespace else {
                continue;
            };
            let bb = b.anchors.last().expect("retained root");
            let existing_a_contains_b = a.missing.is_empty()
                && ancestry[second]
                    .as_ref()
                    .expect("observed")
                    .contains(&anchor_id(aa));
            let existing_b_contains_a = b.missing.is_empty()
                && ancestry[first]
                    .as_ref()
                    .expect("observed")
                    .contains(&anchor_id(bb));
            let missing_overlap = !a.missing.is_empty()
                && !b.missing.is_empty()
                && anchor_id(aa) == anchor_id(bb)
                && (a.missing.starts_with(&b.missing) || b.missing.starts_with(&a.missing));
            if existing_a_contains_b || existing_b_contains_a || missing_overlap {
                return Err(PlanError::Overlap {
                    first: a.destination().to_owned(),
                    second: b.destination().to_owned(),
                });
            }
        }
    }
    Ok(())
}

fn existing_name_collision(
    snapshot: &DestinationSnapshot,
    budget: &mut Budget,
) -> Result<Option<(PathBuf, PathBuf)>, PlanError> {
    let Some(wanted) = snapshot.missing.first() else {
        return Ok(None);
    };
    let parent = snapshot.anchors.last().expect("retained root");
    let key = component_key(wanted);
    let mut collisions = Vec::new();
    for entry in directory_names(parent, budget, "enumerate collision names")? {
        let name = entry.map_err(|error| {
            PlanError::Namespace(NamespaceError::io(
                &parent.path,
                "read collision name",
                error,
            ))
        })?;
        budget.entry()?;
        if component_key(&name) == key && &name != wanted {
            let path = parent.path.join(&name);
            budget.path(&path)?;
            collisions.push(path);
        }
    }
    collisions.sort();
    Ok(collisions
        .into_iter()
        .next()
        .map(|other| (parent.path.join(wanted), other)))
}

fn reject_missing_prefix_collisions(
    destinations: &[PlannedDestination],
    budget: &mut Budget,
) -> Result<(), PlanError> {
    let mut seen: BTreeMap<(Identity, Vec<ComponentKey>), (Vec<OsString>, PathBuf)> =
        BTreeMap::new();
    for destination in destinations {
        let Some(snapshot) = &destination.namespace else {
            continue;
        };
        let parent = snapshot.anchors.last().expect("retained root");
        let mut prefix = Vec::new();
        let mut path = parent.path.clone();
        for name in &snapshot.missing {
            prefix.push(name.clone());
            path.push(name);
            budget.path(&path)?;
            let key = (
                anchor_id(parent),
                prefix.iter().map(|part| component_key(part)).collect(),
            );
            if let Some((previous, other)) = seen.get(&key) {
                if previous != &prefix {
                    return Err(PlanError::PortableNameCollision {
                        first: other.clone(),
                        second: path,
                    });
                }
            } else {
                seen.insert(key, (prefix.clone(), path.clone()));
            }
        }
    }
    Ok(())
}

fn intended_writes(
    sources: &[ValidatedSkill],
    destinations: &mut [PlannedDestination],
    budget: &mut Budget,
) -> Result<Vec<IntendedWrite>, PlanError> {
    let mut unique: BTreeMap<WriteKey, IntendedWrite> = BTreeMap::new();
    for (index, destination) in destinations.iter_mut().enumerate() {
        if destination.action != PlannedAction::Create {
            continue;
        }
        let snapshot = destination
            .namespace
            .as_ref()
            .expect("create requires observed absence");
        let parent = snapshot.anchors.last().expect("retained root");
        let mut path = parent.path.clone();
        let mut prefix = Vec::new();
        for name in snapshot.missing.iter().take(snapshot.missing.len() - 1) {
            path.push(name);
            prefix.push(name.clone());
            let write = IntendedWrite {
                path: path.clone(),
                kind: IntendedWriteKind::CreateParent,
                mode: 0o700,
                bytes: 0,
                digest: None,
            };
            let key = WriteKey::Parent(anchor_id(parent), prefix.clone());
            if let Some(previous) = unique.get_mut(&key) {
                if write.path < previous.path {
                    *previous = write.clone();
                }
            } else {
                budget.write(&write.path)?;
                unique.insert(key, write.clone());
            }
            destination.writes.push(write);
        }
        let source = &sources[destination.source_index];
        let root = snapshot.destination();
        let root_write = IntendedWrite {
            path: root.to_owned(),
            kind: IntendedWriteKind::CreateSkillRoot,
            mode: source.inventory().root_stamp().mode & 0o755,
            bytes: 0,
            digest: None,
        };
        budget.write(&root_write.path)?;
        unique.insert(WriteKey::Payload(index, PathBuf::new()), root_write.clone());
        destination.writes.push(root_write);
        for entry in source.inventory().entries() {
            let write = IntendedWrite {
                path: root.join(&entry.relative_path),
                kind: if entry.kind == SourceEntryKind::File {
                    IntendedWriteKind::CopyFile
                } else {
                    IntendedWriteKind::CreateDirectory
                },
                mode: entry.output_mode,
                bytes: if entry.kind == SourceEntryKind::File {
                    entry.stamp.length
                } else {
                    0
                },
                digest: entry.digest,
            };
            budget.write(&write.path)?;
            unique.insert(
                WriteKey::Payload(index, entry.relative_path.clone()),
                write.clone(),
            );
            destination.writes.push(write);
        }
    }
    let mut writes = unique.into_values().collect::<Vec<_>>();
    writes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(writes)
}

fn directory_names(
    anchor: &DirectoryAnchor,
    budget: &Budget,
    operation: &'static str,
) -> Result<native::Names, PlanError> {
    native::names(
        &anchor.handle,
        budget.limits.max_inspected_entries - budget.entries,
    )
    .map_err(|error| {
        if error.kind() == std::io::ErrorKind::FileTooLarge {
            PlanError::Limit("inspected entries")
        } else {
            PlanError::Namespace(NamespaceError::io(&anchor.path, operation, error))
        }
    })
}
