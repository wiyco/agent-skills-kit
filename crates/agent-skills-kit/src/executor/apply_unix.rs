use super::*;
use crate::{
    filesystem::{flags, stamp},
    planner::{DestinationSnapshot, PlannedAction, PlannedDestination},
    validation::{
        EntryStamp, SourceEntryKind, ValidatedSkill, inventory_retained, inventory_skill,
    },
};
use rustix::fs::{AtFlags, Mode, OFlags, RenameFlags};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    fs::File,
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[path = "copy_unix.rs"]
mod copy;
#[path = "permissions_unix.rs"]
mod permissions;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    BeforeDestination,
    BeforeCopyFile,
    AfterWriteChunk,
    AfterSourceRead,
    AfterStage,
    BeforeCommit,
    BeforeCleanup,
}
fn checkpoint(
    hook: &mut impl FnMut(Phase, &Path) -> io::Result<()>,
    phase: Phase,
    path: &Path,
) -> Result<(), ApplyError> {
    hook(phase, path).map_err(|observed| io_error(path, "native operation checkpoint", observed))
}

type Identity = (u64, u64);
fn id(stamp: &EntryStamp) -> Identity {
    (stamp.device, stamp.inode)
}
fn error(path: &Path, kind: ApplyErrorKind) -> ApplyError {
    ApplyError {
        path: path.to_owned(),
        kind,
    }
}
fn io_error(path: &Path, operation: &'static str, observed: impl Into<io::Error>) -> ApplyError {
    let observed = observed.into();
    error(
        path,
        ApplyErrorKind::Io {
            operation,
            kind: observed.kind(),
            raw_os_error: observed.raw_os_error(),
        },
    )
}
fn metadata(file: &File, path: &Path) -> Result<EntryStamp, ApplyError> {
    file.metadata()
        .map(|meta| stamp(&meta))
        .map_err(|observed| io_error(path, "inspect held object", observed))
}
fn ensure_no_acl(file: &File, path: &Path) -> Result<(), ApplyError> {
    if permissions::no_acl(file)
        .map_err(|observed| io_error(path, "inspect native ACL", observed))?
    {
        Ok(())
    } else {
        Err(error(
            path,
            ApplyErrorKind::Unsupported("ACL-bearing creation parent or output"),
        ))
    }
}
fn bound(parent: &File, name: &OsStr, file: &File, path: &Path) -> Result<bool, ApplyError> {
    let observed = match rustix::fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(value) => value,
        Err(rustix::io::Errno::NOENT) => return Ok(false),
        Err(observed) => return Err(io_error(path, "inspect owned binding", observed)),
    };
    Ok(crate::filesystem::identity_matches(
        &observed,
        &metadata(file, path)?,
    ))
}

struct Owned {
    parent: Arc<File>,
    name: OsString,
    file: Arc<File>,
    path: PathBuf,
    kind: SourceEntryKind,
    identity: Identity,
}
impl Owned {
    fn still_owned(&self) -> Result<bool, ApplyError> {
        Ok(id(&metadata(&self.file, &self.path)?) == self.identity
            && bound(&self.parent, &self.name, &self.file, &self.path)?)
    }
    fn remove(&self) -> Result<(), ApplyError> {
        if !self.still_owned()? {
            return Err(error(&self.path, ApplyErrorKind::StageChanged));
        }
        let flags = if self.kind == SourceEntryKind::Directory {
            AtFlags::REMOVEDIR
        } else {
            AtFlags::empty()
        };
        rustix::fs::unlinkat(&self.parent, self.name.as_os_str(), flags).map_err(|observed| {
            io_error(&self.path, "remove identity-checked owned object", observed)
        })
    }
}

struct Batch {
    parents: Vec<Owned>,
    parent_keys: BTreeMap<(Identity, OsString), usize>,
    committed_parents: BTreeSet<usize>,
}
impl Batch {
    fn new() -> Self {
        Self {
            parents: Vec::new(),
            parent_keys: BTreeMap::new(),
            committed_parents: BTreeSet::new(),
        }
    }
    fn parent(
        &mut self,
        snapshot: &DestinationSnapshot,
        cleanup: &mut Vec<ApplyError>,
    ) -> Result<(Arc<File>, Vec<usize>), ApplyError> {
        let (retained, _) = snapshot.retained_anchor();
        let mut current = Arc::clone(retained);
        let mut used = Vec::new();
        let mut label = snapshot.destination().to_owned();
        for _ in snapshot.missing_components() {
            label.pop();
        }
        for name in snapshot
            .missing_components()
            .iter()
            .take(snapshot.missing_components().len() - 1)
        {
            label.push(name);
            let key = (id(&metadata(&current, &label)?), name.clone());
            if let Some(index) = self.parent_keys.get(&key) {
                used.push(*index);
                let owned = &self.parents[*index];
                if !owned.still_owned()? {
                    return Err(error(&label, ApplyErrorKind::DestinationChanged));
                }
                current = Arc::clone(&owned.file);
            } else {
                let owned = create_directory(&current, name, &label, cleanup)?;
                current = Arc::clone(&owned.file);
                used.push(self.parents.len());
                self.parent_keys.insert(key, self.parents.len());
                self.parents.push(owned);
            }
        }
        Ok((current, used))
    }
    fn report_uncommitted_parents(&self, report: &mut InstallReport) {
        for (index, owned) in self.parents.iter().enumerate().rev() {
            if self.committed_parents.contains(&index) {
                continue;
            }
            // Outside operation-private staging, a binding-check then unlink
            // cannot condition deletion atomically on inode identity. Retain
            // unused created parents instead of risking a substituted entry.
            report
                .cleanup_errors
                .push(error(&owned.path, ApplyErrorKind::CreatedParentRetained));
        }
    }
}

fn create_directory(
    parent: &Arc<File>,
    name: &OsStr,
    path: &Path,
    cleanup: &mut Vec<ApplyError>,
) -> Result<Owned, ApplyError> {
    ensure_no_acl(parent, path)?;
    rustix::fs::mkdirat(parent, name, Mode::from_raw_mode(0o700))
        .map_err(|observed| io_error(path, "create exclusive private directory", observed))?;
    let observed =
        rustix::fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|observed| {
            let failure = io_error(path, "inspect newly created directory", observed);
            cleanup.push(failure.clone());
            failure
        })?;
    let descriptor =
        match rustix::fs::openat(parent, name, flags() | OFlags::DIRECTORY, Mode::empty()) {
            Ok(file) => file,
            Err(observed) => {
                let failure = io_error(
                    path,
                    "open newly created directory; ownership unconfirmed",
                    observed,
                );
                cleanup.push(failure.clone());
                return Err(failure);
            }
        };
    let file = Arc::new(File::from(descriptor));
    let stamp = metadata(&file, path).inspect_err(|failure| {
        cleanup.push(failure.clone());
    })?;
    let current_binding = bound(parent, name, &file, path).inspect_err(|failure| {
        cleanup.push(failure.clone());
    })?;
    if !crate::filesystem::binding_matches(&observed, &stamp) || !current_binding {
        let failure = error(path, ApplyErrorKind::StageChanged);
        cleanup.push(failure.clone());
        return Err(failure);
    }
    let owned = Owned {
        parent: Arc::clone(parent),
        name: name.to_owned(),
        file,
        path: path.to_owned(),
        kind: SourceEntryKind::Directory,
        identity: id(&stamp),
    };
    if let Err(failure) = ensure_no_acl(&owned.file, path).and_then(|()| {
        rustix::fs::fchmod(&owned.file, Mode::from_raw_mode(0o700))
            .map_err(|observed| io_error(path, "set private directory mode", observed))
    }) {
        // This helper also creates destination parents outside private staging.
        // Do not turn a failed setup into check-then-unlink authority there.
        cleanup.push(failure.clone());
        return Err(failure);
    }
    Ok(owned)
}

fn source_current(
    source: &ValidatedSkill,
    limits: crate::validation::InventoryLimits,
) -> Result<(), ApplyError> {
    let path = source.inventory().requested_root();
    let current = inventory_skill(path, limits)
        .map_err(|failure| error(path, ApplyErrorKind::SourceInspection(failure)))?;
    if source.inventory().root_stamp() != current.inventory().root_stamp()
        || source.inventory().entries() != current.inventory().entries()
        || source.inventory().canonical_root() != current.inventory().canonical_root()
        || source.metadata().source_bytes() != current.metadata().source_bytes()
    {
        return Err(error(path, ApplyErrorKind::SourceChanged));
    }
    Ok(())
}
fn bindings(destination: &PlannedDestination) -> Result<(), ApplyError> {
    let path = destination.path().expect("applicable path");
    if !destination
        .namespace()
        .expect("applicable namespace")
        .existing_bindings_are_current()
        .map_err(|failure| error(path, ApplyErrorKind::Namespace(failure)))?
    {
        return Err(error(path, ApplyErrorKind::DestinationChanged));
    }
    Ok(())
}

pub(super) fn apply(plan: &InstallPlan, options: ApplyOptions, report: &mut InstallReport) {
    apply_with_hook(plan, options, report, &mut |_, _| Ok(()));
}

fn apply_with_hook(
    plan: &InstallPlan,
    options: ApplyOptions,
    report: &mut InstallReport,
    hook: &mut impl FnMut(Phase, &Path) -> io::Result<()>,
) {
    let mut batch = Batch::new();
    for (index, destination) in plan.destinations().iter().enumerate() {
        let source = &plan.sources()[destination.source_index()];
        let result = checkpoint(
            hook,
            Phase::BeforeDestination,
            destination.path().expect("applicable path"),
        )
        .and_then(|()| source_current(source, options.verification.inventory))
        .and_then(|()| bindings(destination))
        .and_then(|()| {
            if destination.action() == PlannedAction::Unchanged {
                let expected = destination.existing_payload().expect("unchanged snapshot");
                let current = inventory_retained(
                    Arc::clone(expected.inventory().retained_root()),
                    expected.inventory().canonical_root(),
                    expected.metadata().name(),
                    options.verification.inventory,
                )
                .map_err(|failure| {
                    error(
                        destination.path().expect("path"),
                        ApplyErrorKind::DestinationInspection(failure),
                    )
                })?;
                if expected.inventory().root_stamp() != current.inventory().root_stamp()
                    || expected.inventory().entries() != current.inventory().entries()
                    || expected.metadata().source_bytes() != current.metadata().source_bytes()
                {
                    return Err(error(
                        destination.path().expect("path"),
                        ApplyErrorKind::DestinationChanged,
                    ));
                }
                bindings(destination)?;
                Ok(OutcomeStatus::Unchanged)
            } else {
                install(
                    source,
                    destination,
                    options,
                    &mut batch,
                    &mut report.cleanup_errors,
                    hook,
                )
                .map(|()| OutcomeStatus::Created)
            }
        });
        let failed = result.is_err();
        report.outcomes[index].status = match result {
            Ok(status) => status,
            Err(failure) => OutcomeStatus::Failed(failure),
        };
        if failed && options.stop_on_failure {
            break;
        }
    }
    batch.report_uncommitted_parents(report);
}

fn install(
    source: &ValidatedSkill,
    destination: &PlannedDestination,
    options: ApplyOptions,
    batch: &mut Batch,
    cleanup: &mut Vec<ApplyError>,
    hook: &mut impl FnMut(Phase, &Path) -> io::Result<()>,
) -> Result<(), ApplyError> {
    let snapshot = destination.namespace().expect("create snapshot");
    let path = destination.path().expect("create path");
    let (parent, used_parents) = batch.parent(snapshot, cleanup)?;
    let leaf = snapshot
        .missing_components()
        .last()
        .expect("create absent leaf");
    // Never adopt an entry created since planning, even if its bytes now match.
    match rustix::fs::statat(&parent, leaf.as_os_str(), AtFlags::SYMLINK_NOFOLLOW) {
        Err(rustix::io::Errno::NOENT) => (),
        Ok(_) => return Err(error(path, ApplyErrorKind::DestinationChanged)),
        Err(observed) => return Err(io_error(path, "check selected absence", observed)),
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut stage = None;
    for _ in 0..100 {
        let name = OsString::from(format!(
            ".ask-stage-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let label = path.parent().expect("destination parent").join(&name);
        match create_directory(&parent, &name, &label, cleanup) {
            Ok(owned) => {
                stage = Some(owned);
                break;
            }
            Err(failure)
                if matches!(
                    failure.kind,
                    ApplyErrorKind::Io {
                        raw_os_error: Some(libc::EEXIST),
                        ..
                    }
                ) =>
            {
                continue;
            }
            Err(failure) => return Err(failure),
        }
    }
    let stage = stage.ok_or_else(|| {
        error(
            path,
            ApplyErrorKind::Unsupported("exclusive staging name budget exhausted"),
        )
    })?;
    let mut objects = Vec::new();
    let result = (|| {
        let payload = create_directory(
            &stage.file,
            OsStr::new(source.metadata().name()),
            &stage.path.join(source.metadata().name()),
            cleanup,
        )?;
        let payload_root = Arc::clone(&payload.file);
        let payload_name = payload.name.clone();
        objects.push(payload);
        copy::payload(
            source,
            &payload_root,
            &stage.path.join(&payload_name),
            &mut objects,
            cleanup,
            hook,
        )?;
        let verified_stage = metadata(&stage.file, &stage.path)?;
        let verified_objects = objects
            .iter()
            .map(|owned| metadata(&owned.file, &owned.path))
            .collect::<Result<Vec<_>, _>>()?;
        checkpoint(hook, Phase::AfterStage, &stage.path)?;
        source_current(source, options.verification.inventory)?;
        bindings(destination)?;
        for owned in &batch.parents {
            if !owned.still_owned()? {
                return Err(error(&owned.path, ApplyErrorKind::DestinationChanged));
            }
        }
        if !stage.still_owned()? || metadata(&stage.file, &stage.path)? != verified_stage {
            return Err(error(&stage.path, ApplyErrorKind::StageChanged));
        }
        for (owned, verified) in objects.iter().zip(verified_objects) {
            if !owned.still_owned()? || metadata(&owned.file, &owned.path)? != verified {
                return Err(error(&owned.path, ApplyErrorKind::StageChanged));
            }
        }
        checkpoint(hook, Phase::BeforeCommit, path)?;
        rustix::fs::renameat_with(
            &stage.file,
            payload_name.as_os_str(),
            &parent,
            leaf.as_os_str(),
            RenameFlags::NOREPLACE,
        )
        .map_err(|observed| io_error(path, "native exclusive directory commit", observed))?;
        // Ownership of the installed payload is no longer cleanup authority.
        objects.clear();
        batch.committed_parents.extend(used_parents);
        Ok(())
    })();
    if let Err(failure) = checkpoint(hook, Phase::BeforeCleanup, &stage.path) {
        cleanup.push(failure);
        return result;
    }
    for owned in std::iter::once(&stage).chain(&objects) {
        match owned.still_owned() {
            Ok(true) => (),
            Ok(false) => {
                cleanup.push(error(&owned.path, ApplyErrorKind::StageChanged));
                return result;
            }
            Err(failure) => {
                cleanup.push(failure);
                return result;
            }
        }
    }
    for owned in &objects {
        if owned.kind == SourceEntryKind::Directory && owned.still_owned().unwrap_or(false) {
            if let Err(observed) = rustix::fs::fchmod(&owned.file, Mode::from_raw_mode(0o700)) {
                cleanup.push(io_error(
                    &owned.path,
                    "restore owned staging cleanup access",
                    observed,
                ));
            }
        }
    }
    for owned in objects.iter().rev() {
        if let Err(failure) = owned.remove() {
            cleanup.push(failure);
        }
    }
    if let Err(failure) = stage.remove() {
        cleanup.push(failure);
    }
    result
}

#[cfg(test)]
#[path = "apply_tests.rs"]
mod tests;
