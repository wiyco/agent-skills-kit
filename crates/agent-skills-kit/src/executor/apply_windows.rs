//! Retained-handle Windows copy backend with bounded native regression coverage.
use super::*;
use crate::{
    filesystem_windows::{
        self as native, Kind,
        security::{self, creation},
    },
    planner::{DestinationSnapshot, PlannedAction, PlannedDestination},
    validation::{
        EntryStamp, InventoryLimits, SourceEntryKind, ValidatedSkill, inventory_retained,
        inventory_skill,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    fs::File,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[path = "copy_windows.rs"]
mod copy;

type Identity = (u64, [u8; 16]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    BeforeDestination,
    BeforeCopyFile,
    AfterWriteChunk,
    AfterSourceRead,
    AfterStage,
    BeforeCommit,
    AfterCommit,
    BeforePermissions,
    BeforeCleanup,
}
fn error(path: &Path, kind: ApplyErrorKind) -> ApplyError {
    ApplyError {
        path: path.to_owned(),
        kind,
    }
}
fn io_error(path: &Path, operation: &'static str, observed: io::Error) -> ApplyError {
    let original = observed
        .get_ref()
        .and_then(|cause| cause.downcast_ref::<native::UnconfirmedCreation>())
        .map_or(&observed, |cause| &cause.0);
    error(
        path,
        ApplyErrorKind::Io {
            operation,
            kind: original.kind(),
            raw_os_error: original.raw_os_error(),
        },
    )
}
fn checkpoint(
    hook: &mut impl FnMut(Phase, &Path) -> io::Result<()>,
    phase: Phase,
    path: &Path,
) -> Result<(), ApplyError> {
    hook(phase, path).map_err(|observed| io_error(path, "native operation checkpoint", observed))
}
fn metadata(file: &File, path: &Path) -> Result<EntryStamp, ApplyError> {
    native::entry_stamp(file)
        .map_err(|observed| io_error(path, "inspect held Windows object", observed))
}
fn descriptor(file: &File, path: &Path) -> Result<Vec<u8>, ApplyError> {
    security::descriptor(file)
        .map_err(|observed| io_error(path, "inspect owned security descriptor", observed))
}
fn kind(entry: SourceEntryKind) -> Kind {
    match entry {
        SourceEntryKind::Directory => Kind::Directory,
        SourceEntryKind::File => Kind::File,
    }
}
fn bound(
    parent: &File,
    name: &OsStr,
    file: &File,
    entry: Kind,
    path: &Path,
) -> Result<bool, ApplyError> {
    let reopened = match native::open_binding(parent, name, entry) {
        Ok(file) => file,
        Err(observed) if observed.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(observed) => return Err(io_error(path, "recheck held object binding", observed)),
    };
    Ok(metadata(&reopened, path)?.file_identity() == metadata(file, path)?.file_identity())
}
struct Context {
    token: creation::Token,
    stage: creation::Descriptor,
    directory: creation::Descriptor,
    file: creation::Descriptor,
}
impl Context {
    fn new(path: &Path) -> Result<Self, ApplyError> {
        let token = creation::Token::effective()
            .map_err(|observed| io_error(path, "query effective creation identity", observed))?;
        let stage = creation::Descriptor::private_for(&token)
            .map_err(|observed| io_error(path, "prepare private creation descriptor", observed))?;
        let directory = creation::Descriptor::private_payload(&token)
            .map_err(|observed| io_error(path, "prepare private payload descriptor", observed))?;
        let file = creation::Descriptor::private_file(&token)
            .map_err(|observed| io_error(path, "prepare private file descriptor", observed))?;
        Ok(Self {
            token,
            stage,
            directory,
            file,
        })
    }
}
fn create(
    parent: &File,
    name: &OsStr,
    entry: Kind,
    security: &creation::Descriptor,
    path: &Path,
    cleanup: &mut Vec<ApplyError>,
) -> Result<File, ApplyError> {
    native::create_with_security(parent, name, entry, security).map_err(|observed| {
        let unconfirmed = observed
            .get_ref()
            .is_some_and(|cause| cause.is::<native::UnconfirmedCreation>());
        let failure = io_error(path, "exclusive private creation", observed);
        if unconfirmed {
            cleanup.push(failure.clone());
        }
        failure
    })
}
struct OwnedDirectory {
    parent: Arc<File>,
    name: OsString,
    file: Arc<File>,
    path: PathBuf,
    identity: Identity,
    security: Vec<u8>,
}
impl OwnedDirectory {
    fn create(
        parent: &Arc<File>,
        name: &OsStr,
        path: &Path,
        context: &Context,
        cleanup: &mut Vec<ApplyError>,
    ) -> Result<Self, ApplyError> {
        let file = Arc::new(create(
            parent,
            name,
            Kind::Directory,
            &context.stage,
            path,
            cleanup,
        )?);
        let identity = metadata(&file, path)
            .inspect_err(|failure| cleanup.push(failure.clone()))?
            .file_identity();
        let security =
            descriptor(&file, path).inspect_err(|failure| cleanup.push(failure.clone()))?;
        Ok(Self {
            parent: Arc::clone(parent),
            name: name.to_owned(),
            file,
            path: path.to_owned(),
            identity,
            security,
        })
    }
    fn current(&self) -> Result<bool, ApplyError> {
        Ok(
            metadata(&self.file, &self.path)?.file_identity() == self.identity
                && metadata(&self.file, &self.path)?.mode & 0o200 != 0
                && descriptor(&self.file, &self.path)? == self.security
                && bound(
                    &self.parent,
                    &self.name,
                    &self.file,
                    Kind::Directory,
                    &self.path,
                )?,
        )
    }
}
struct Batch {
    parents: Vec<OwnedDirectory>,
    keys: BTreeMap<(Identity, OsString), usize>,
    committed: BTreeSet<usize>,
}
impl Batch {
    fn new() -> Self {
        Self {
            parents: vec![],
            keys: BTreeMap::new(),
            committed: BTreeSet::new(),
        }
    }
    fn parent(
        &mut self,
        snapshot: &DestinationSnapshot,
        context: &Context,
        cleanup: &mut Vec<ApplyError>,
    ) -> Result<(Arc<File>, Vec<usize>), ApplyError> {
        let (anchor, _) = snapshot.retained_anchor();
        let mut current = Arc::new(anchor.try_clone().map_err(|observed| {
            io_error(
                snapshot.destination(),
                "retain trusted destination handle",
                observed,
            )
        })?);
        let mut label = snapshot.destination().to_owned();
        for _ in snapshot.missing_components() {
            label.pop();
        }
        let mut used = vec![];
        for name in snapshot
            .missing_components()
            .iter()
            .take(snapshot.missing_components().len() - 1)
        {
            label.push(name);
            let key = (metadata(&current, &label)?.file_identity(), name.clone());
            if let Some(index) = self.keys.get(&key) {
                let owned = &self.parents[*index];
                if !owned.current()? {
                    return Err(error(&label, ApplyErrorKind::DestinationChanged));
                }
                current = Arc::clone(&owned.file);
                used.push(*index);
            } else {
                let owned = OwnedDirectory::create(&current, name, &label, context, cleanup)?;
                current = Arc::clone(&owned.file);
                let index = self.parents.len();
                self.parents.push(owned);
                self.keys.insert(key, index);
                used.push(index);
            }
        }
        Ok((current, used))
    }
    fn current(&self) -> Result<(), ApplyError> {
        for parent in &self.parents {
            if !parent.current()? {
                return Err(error(&parent.path, ApplyErrorKind::DestinationChanged));
            }
        }
        Ok(())
    }
    fn report_retained(&self, cleanup: &mut Vec<ApplyError>) {
        for (index, parent) in self.parents.iter().enumerate() {
            if !self.committed.contains(&index) {
                cleanup.push(error(&parent.path, ApplyErrorKind::CreatedParentRetained));
            }
        }
    }
}
struct Record {
    relative: PathBuf,
    entry: SourceEntryKind,
    identity: Identity,
    stamp: EntryStamp,
    security: Vec<u8>,
    digest: Option<[u8; 32]>,
}
struct Payload {
    records: Vec<Record>,
    live: BTreeMap<PathBuf, Arc<File>>,
    committed: bool,
}
impl Payload {
    fn record(
        &mut self,
        relative: &Path,
        entry: SourceEntryKind,
        file: File,
        label: &Path,
        digest: Option<[u8; 32]>,
    ) -> Result<(), ApplyError> {
        let stamp = metadata(&file, label)?;
        let security = descriptor(&file, label)?;
        self.records.push(Record {
            relative: relative.to_owned(),
            entry,
            identity: stamp.file_identity(),
            stamp,
            security,
            digest,
        });
        self.live.insert(relative.to_owned(), Arc::new(file));
        Ok(())
    }
    fn capture(&mut self, label: &Path) -> Result<(), ApplyError> {
        for record in &mut self.records {
            record.stamp = metadata(&self.live[&record.relative], &label.join(&record.relative))?;
        }
        Ok(())
    }
    fn verify(&self, label: &Path) -> Result<(), ApplyError> {
        let mut children: BTreeMap<PathBuf, BTreeSet<OsString>> = BTreeMap::new();
        for record in self.records.iter().skip(1) {
            children
                .entry(
                    record
                        .relative
                        .parent()
                        .expect("relative parent")
                        .to_owned(),
                )
                .or_default()
                .insert(record.relative.file_name().expect("entry name").to_owned());
        }
        for record in &self.records {
            let path = label.join(&record.relative);
            let file = &self.live[&record.relative];
            if metadata(file, &path)? != record.stamp || descriptor(file, &path)? != record.security
            {
                return Err(error(&path, ApplyErrorKind::StageChanged));
            }
            if record.entry == SourceEntryKind::File {
                copy::verify(record, file, &path)?;
            }
            if !record.relative.as_os_str().is_empty() {
                let parent = &self.live[record.relative.parent().expect("relative entry parent")];
                if !bound(
                    parent,
                    record.relative.file_name().expect("relative entry name"),
                    file,
                    kind(record.entry),
                    &path,
                )? {
                    return Err(error(&path, ApplyErrorKind::StageChanged));
                }
            }
            if record.entry == SourceEntryKind::Directory {
                let expected = children.get(&record.relative).cloned().unwrap_or_default();
                let scan = native::reader::Handle::new(file.try_clone().map_err(|observed| {
                    io_error(&path, "retain stage enumeration handle", observed)
                })?);
                let found = scan
                    .entries(expected.len())
                    .map_err(|observed| io_error(&path, "enumerate bounded owned stage", observed))?
                    .into_iter()
                    .map(|entry| entry.name)
                    .collect::<BTreeSet<_>>();
                if found != expected {
                    return Err(error(&path, ApplyErrorKind::StageChanged));
                }
            }
        }
        Ok(())
    }
    fn permissions(
        &self,
        root: &Arc<File>,
        parent: &File,
        path: &Path,
        context: &Context,
    ) -> Result<(), ApplyError> {
        let expected_root = &self.records[0];
        if metadata(root, path)?.file_identity() != expected_root.identity
            || metadata(root, path)?.mode != expected_root.stamp.mode
            || descriptor(root, path)? != expected_root.security
        {
            return Err(error(path, ApplyErrorKind::StageChanged));
        }
        let mut held = BTreeMap::from([(PathBuf::new(), Arc::clone(root))]);
        for record in self.records.iter().skip(1) {
            let parent = &held[record.relative.parent().expect("relative parent")];
            let label = path.join(&record.relative);
            let file = native::open_permissions(
                parent,
                record.relative.file_name().expect("entry name"),
                kind(record.entry),
            )
            .map_err(|observed| {
                io_error(&label, "reacquire private permission handle", observed)
            })?;
            if metadata(&file, &label)?.file_identity() != record.identity
                || descriptor(&file, &label)? != record.security
            {
                return Err(error(&label, ApplyErrorKind::StageChanged));
            }
            held.insert(record.relative.clone(), Arc::new(file));
        }
        for record in &self.records {
            let label = path.join(&record.relative);
            let parent = if record.relative.as_os_str().is_empty() {
                parent
            } else {
                held[record.relative.parent().unwrap()].as_ref()
            };
            creation::finalize_inheritance(&held[&record.relative], parent, &context.token)
                .map_err(|observed| {
                    io_error(
                        &label,
                        "finalize committed destination permissions",
                        observed,
                    )
                })?;
        }
        Ok(())
    }
}
fn source_current(source: &ValidatedSkill, limits: InventoryLimits) -> Result<(), ApplyError> {
    let path = source.inventory().requested_root();
    let current = inventory_skill(path, limits)
        .map_err(|observed| error(path, ApplyErrorKind::SourceInspection(observed)))?;
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
        .map_err(|observed| error(path, ApplyErrorKind::Namespace(observed)))?
    {
        return Err(error(path, ApplyErrorKind::DestinationChanged));
    }
    Ok(())
}
fn cleanup_payload(
    stage: &OwnedDirectory,
    payload: &mut Payload,
    name: &OsStr,
    label: &Path,
) -> Result<(), ApplyError> {
    payload.live.clear();
    if payload.committed || payload.records.is_empty() {
        return Ok(());
    }
    if !stage.current()? {
        return Err(error(&stage.path, ApplyErrorKind::StageChanged));
    }
    let root = native::open_cleanup(&stage.file, name, Kind::Directory)
        .map_err(|observed| io_error(label, "reacquire owned cleanup root", observed))?;
    let mut held = BTreeMap::from([(PathBuf::new(), Arc::new(root))]);
    for record in &payload.records {
        if !record.relative.as_os_str().is_empty() {
            let parent = &held[record.relative.parent().expect("relative parent")];
            let file = native::open_cleanup(
                parent,
                record.relative.file_name().expect("entry name"),
                kind(record.entry),
            )
            .map_err(|observed| {
                io_error(
                    &label.join(&record.relative),
                    "reacquire recorded cleanup entry",
                    observed,
                )
            })?;
            held.insert(record.relative.clone(), Arc::new(file));
        }
        let file = &held[&record.relative];
        let path = label.join(&record.relative);
        if metadata(file, &path)?.file_identity() != record.identity
            || descriptor(file, &path)? != record.security
        {
            return Err(error(&path, ApplyErrorKind::StageChanged));
        }
    }
    for record in payload.records.iter().rev() {
        let file = held
            .remove(&record.relative)
            .expect("verified cleanup entry");
        let path = label.join(&record.relative);
        native::set_readonly(&file, false)
            .map_err(|observed| io_error(&path, "restore owned cleanup attribute", observed))?;
        native::dispose_verified(
            file.try_clone()
                .map_err(|observed| io_error(&path, "retain cleanup capability", observed))?,
        )
        .map_err(|observed| io_error(&path, "delete recorded owned object", observed))?;
        drop(file);
    }
    Ok(())
}
fn install(
    source: &ValidatedSkill,
    destination: &PlannedDestination,
    options: ApplyOptions,
    context: &Context,
    batch: &mut Batch,
    cleanup: &mut Vec<ApplyError>,
    hook: &mut impl FnMut(Phase, &Path) -> io::Result<()>,
) -> Result<OutcomeStatus, ApplyError> {
    let path = destination.path().expect("create path");
    let snapshot = destination.namespace().unwrap();
    let (parent, used) = batch.parent(snapshot, context, cleanup)?;
    let leaf = snapshot
        .missing_components()
        .last()
        .expect("absent skill leaf");
    match native::open_binding(&parent, leaf, Kind::Directory) {
        Err(observed) if observed.kind() == io::ErrorKind::NotFound => (),
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
        let label = path.parent().unwrap().join(&name);
        match OwnedDirectory::create(&parent, &name, &label, context, cleanup) {
            Ok(owned) => {
                stage = Some(owned);
                break;
            }
            Err(failure)
                if matches!(
                    failure.kind,
                    ApplyErrorKind::Io {
                        kind: io::ErrorKind::AlreadyExists,
                        ..
                    }
                ) => {}
            Err(failure) => return Err(failure),
        }
    }
    let stage = stage.ok_or_else(|| {
        error(
            path,
            ApplyErrorKind::Unsupported("exclusive staging name budget"),
        )
    })?;
    let name = OsStr::new(source.metadata().name());
    let label = stage.path.join(name);
    let mut payload = Payload {
        records: vec![],
        live: BTreeMap::new(),
        committed: false,
    };
    let result = (|| {
        let root = create(
            &stage.file,
            name,
            Kind::Directory,
            &context.directory,
            &label,
            cleanup,
        )?;
        payload.record(
            Path::new(""),
            SourceEntryKind::Directory,
            root,
            &label,
            None,
        )?;
        copy::payload(source, &label, context, &mut payload, cleanup, hook)?;
        payload.capture(&label)?;
        checkpoint(hook, Phase::AfterStage, &stage.path)?;
        source_current(source, options.verification.inventory)?;
        bindings(destination)?;
        batch.current()?;
        if !stage.current()? {
            return Err(error(&stage.path, ApplyErrorKind::StageChanged));
        }
        payload.verify(&label)?;
        let root = payload
            .live
            .remove(Path::new(""))
            .expect("retained payload root");
        payload.live.clear();
        checkpoint(hook, Phase::BeforeCommit, path)?;
        native::rename_exclusive(&root, &parent, leaf)
            .map_err(|observed| io_error(path, "native exclusive directory commit", observed))?;
        payload.committed = true;
        batch.committed.extend(used);
        let finalized = checkpoint(hook, Phase::AfterCommit, path)
            .and_then(|()| bindings(destination))
            .and_then(|()| batch.current())
            .and_then(|()| checkpoint(hook, Phase::BeforePermissions, path))
            .and_then(|()| payload.permissions(&root, &parent, path, context));
        Ok(match finalized {
            Ok(()) => OutcomeStatus::Created,
            Err(failure) => OutcomeStatus::CommittedWithPermissionFailure(failure),
        })
    })();
    if let Err(failure) = checkpoint(hook, Phase::BeforeCleanup, &stage.path) {
        cleanup.push(failure);
        return result;
    }
    if let Err(failure) = bindings(destination)
        .and_then(|()| batch.current())
        .and_then(|()| cleanup_payload(&stage, &mut payload, name, &label))
    {
        cleanup.push(failure);
        return result;
    }
    match stage.current() {
        Ok(true) => match stage
            .file
            .try_clone()
            .map_err(|observed| {
                io_error(
                    &stage.path,
                    "retain owned stage cleanup capability",
                    observed,
                )
            })
            .and_then(|file| {
                native::dispose_verified(file)
                    .map_err(|observed| io_error(&stage.path, "remove empty owned stage", observed))
            }) {
            Ok(()) => (),
            Err(failure) => cleanup.push(failure),
        },
        Ok(false) => cleanup.push(error(&stage.path, ApplyErrorKind::StageChanged)),
        Err(failure) => cleanup.push(failure),
    }
    result
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
        let path = destination.path().expect("applicable destination");
        let source = &plan.sources()[destination.source_index()];
        let result = checkpoint(hook, Phase::BeforeDestination, path)
            .and_then(|()| source_current(source, options.verification.inventory))
            .and_then(|()| bindings(destination))
            .and_then(|()| {
                if destination.action() == PlannedAction::Unchanged {
                    let expected = destination.existing_payload().unwrap();
                    let current = inventory_retained(
                        Arc::clone(expected.inventory().retained_root()),
                        path,
                        expected.metadata().name(),
                        options.verification.inventory,
                    )
                    .map_err(|observed| {
                        error(path, ApplyErrorKind::DestinationInspection(observed))
                    })?;
                    if expected.inventory().root_stamp() != current.inventory().root_stamp()
                        || expected.inventory().entries() != current.inventory().entries()
                        || expected.metadata().source_bytes() != current.metadata().source_bytes()
                    {
                        return Err(error(path, ApplyErrorKind::DestinationChanged));
                    }
                    bindings(destination)?;
                    Ok(OutcomeStatus::Unchanged)
                } else {
                    let context = Context::new(path)?;
                    install(
                        source,
                        destination,
                        options,
                        &context,
                        &mut batch,
                        &mut report.cleanup_errors,
                        hook,
                    )
                }
            });
        let status = result.unwrap_or_else(OutcomeStatus::Failed);
        let failed = !matches!(status, OutcomeStatus::Created | OutcomeStatus::Unchanged);
        report.outcomes[index].status = status;
        if failed && options.stop_on_failure {
            break;
        }
    }
    batch.report_retained(&mut report.cleanup_errors);
}

#[cfg(test)]
#[path = "apply_windows_tests.rs"]
mod tests;
