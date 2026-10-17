//! Held-object NTFS removal; no pathname deletion or permission repair fallback.
use super::*;
use crate::{
    filesystem_windows::{
        self as native, Kind,
        reader::Handle,
        security::{self, creation},
    },
    validation::{SourceEntryKind, ValidatedSkill, inventory_retained},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    fs::File,
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
// Keep native error propagation small without changing the public report API.
type Result<T> = std::result::Result<T, Box<RemovalFailure>>;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    BeforeRootOpen,
    BeforeStage,
    AfterStage,
    BeforeDetach,
    AfterDetach,
    BeforeUnlink,
    AfterUnlink,
    BeforeCleanup,
}
fn fail(path: &Path, operation: &'static str, kind: RemovalFailureKind) -> Box<RemovalFailure> {
    Box::new(RemovalFailure {
        path: path.to_owned(),
        operation,
        kind,
    })
}
fn io_error(path: &Path, operation: &'static str, error: io::Error) -> Box<RemovalFailure> {
    fail(
        path,
        operation,
        RemovalFailureKind::Io {
            kind: error.kind(),
            raw_os_error: error.raw_os_error(),
        },
    )
}
fn changed(path: &Path) -> Box<RemovalFailure> {
    fail(
        path,
        "verify selected Windows object/binding",
        RemovalFailureKind::Changed,
    )
}
fn stamp(file: &File, path: &Path) -> Result<EntryStamp> {
    native::entry_stamp(file).map_err(|e| io_error(path, "inspect held Windows removal object", e))
}
fn hook(
    phase: Phase,
    path: &Path,
    run: &mut impl FnMut(Phase, &Path) -> io::Result<()>,
) -> Result<()> {
    run(phase, path).map_err(|e| io_error(path, "Windows removal boundary hook", e))
}
fn kind(entry: SourceEntryKind) -> Kind {
    match entry {
        SourceEntryKind::Directory => Kind::Directory,
        SourceEntryKind::File => Kind::File,
    }
}
fn binding(
    parent: &File,
    name: &OsStr,
    expected: &EntryStamp,
    entry: Kind,
    path: &Path,
) -> Result<()> {
    let opened = native::open_binding(parent, name, entry)
        .map_err(|e| io_error(path, "verify Windows removal binding", e))?;
    if stamp(&opened, path)?.file_identity() != expected.file_identity() {
        return Err(changed(path));
    }
    Ok(())
}
// A different new occupant is never removal authority; the original held object
// can be reported removed only after its handle is closed and its name is gone.
fn removed_binding(
    parent: &File,
    name: &OsStr,
    expected: &EntryStamp,
    entry: Kind,
    path: &Path,
) -> Result<()> {
    match native::open_binding(parent, name, entry) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(
            path,
            "confirm removal after closing the object handle",
            error,
        )),
        Ok(file) => {
            if stamp(&file, path)?.file_identity() != expected.file_identity() {
                Ok(())
            } else {
                Err(changed(path))
            }
        }
    }
}
struct Stage {
    parent: Arc<Handle>,
    file: Handle,
    name: OsString,
    path: PathBuf,
    identity: EntryStamp,
    security: Vec<u8>,
}
impl Stage {
    fn current(&self, plan: &RemovalPlan) -> Result<()> {
        if !plan.namespace_is_current().map_err(|e| {
            fail(
                &self.path,
                "verify removal container",
                RemovalFailureKind::Preflight(e),
            )
        })? {
            return Err(changed(&self.path));
        }
        let current = stamp(&self.file, &self.path)?;
        if current.file_identity() != self.identity.file_identity()
            || current.mode & 0o200 == 0
            || security::descriptor(&self.file)
                .map_err(|e| io_error(&self.path, "verify private enclosure descriptor", e))?
                != self.security
        {
            return Err(changed(&self.path));
        }
        binding(
            &self.parent,
            &self.name,
            &current,
            Kind::Directory,
            &self.path,
        )
    }
    fn cleanup(
        self,
        plan: &RemovalPlan,
        report: &mut RemovalReport,
        run: &mut impl FnMut(Phase, &Path) -> io::Result<()>,
    ) {
        let checked = hook(Phase::BeforeCleanup, &self.path, run).and_then(|()| self.current(plan));
        if let Err(error) = checked {
            report.stage_location_uncertain = true;
            report.unrecognized_scan_incomplete = true;
            report.cleanup_failures.push(*error);
            return;
        }
        let entries = match self.file.entries(plan.limits.inventory.max_entries) {
            Ok(entries) => entries,
            Err(error) => {
                report.unrecognized_scan_incomplete = true;
                report.cleanup_failures.push(*io_error(
                    &self.path,
                    "bounded enclosure cleanup inspection",
                    error,
                ));
                return;
            }
        };
        if !entries.is_empty() {
            let mut bytes_left = 1024 * 1024usize;
            for entry in entries {
                let path = self.path.join(entry.name);
                let bytes = crate::planner::native::path_bytes(&path);
                if bytes > bytes_left {
                    report.unrecognized_scan_incomplete = true;
                    break;
                }
                bytes_left -= bytes;
                report.unrecognized_entries.push(path);
            }
            report.cleanup_failures.push(*changed(&self.path));
            return;
        }
        let result = native::dispose_verified(self.file.into_file())
            .map_err(|e| io_error(&self.path, "dispose verified empty private enclosure", e))
            .and_then(|()| {
                removed_binding(
                    &self.parent,
                    &self.name,
                    &self.identity,
                    Kind::Directory,
                    &self.path,
                )
            });
        match result {
            Ok(()) => {
                report.retained_stage = None;
                report.stage_location_uncertain = false;
            }
            Err(error) => {
                report.cleanup_failures.push(*error);
                report.stage_location_uncertain = true;
                report.unrecognized_scan_incomplete = true;
            }
        }
    }
}
fn create_stage(plan: &RemovalPlan, report: &mut RemovalReport) -> Result<Stage> {
    let parent = Arc::clone(plan.namespace.retained_anchor().0);
    if !plan.namespace_is_current().map_err(|e| {
        fail(
            &plan.path,
            "verify creation namespace",
            RemovalFailureKind::Preflight(e),
        )
    })? {
        return Err(changed(&plan.path));
    }
    let token = creation::Token::effective()
        .map_err(|e| io_error(&plan.path, "query private enclosure identity", e))?;
    let descriptor = creation::Descriptor::private_for(&token)
        .map_err(|e| io_error(&plan.path, "prepare private enclosure descriptor", e))?;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    for _ in 0..100 {
        let name = OsString::from(format!(
            ".ask-remove-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let path = plan.namespace.destination().join(&name);
        let file = match native::create_with_security(&parent, &name, Kind::Directory, &descriptor)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                if error
                    .get_ref()
                    .is_some_and(|cause| cause.is::<native::UnconfirmedCreation>())
                {
                    report.retained_stage = Some(path.clone());
                    report.stage_location_uncertain = true;
                }
                return Err(io_error(
                    &path,
                    "exclusive private Windows enclosure creation",
                    error,
                ));
            }
        };
        report.retained_stage = Some(path.clone());
        report.stage_location_uncertain = true;
        let identity = stamp(&file, &path)?;
        let security = security::descriptor(&file)
            .map_err(|e| io_error(&path, "capture private enclosure descriptor", e))?;
        let stage = Stage {
            parent,
            file: Handle::new(file),
            name,
            path,
            identity,
            security,
        };
        stage.current(plan)?;
        report.stage_location_uncertain = false;
        return Ok(stage);
    }
    Err(fail(
        &plan.path,
        "private enclosure creation",
        RemovalFailureKind::Unsupported("exclusive stage-name collision budget exhausted"),
    ))
}
fn same_payload(plan: &RemovalPlan, current: &ValidatedSkill, detached: bool) -> bool {
    let mut root = current.inventory().root_stamp().clone();
    if detached {
        root.changed = plan.root_stamp.changed;
        root.windows.changed = plan.root_stamp.windows.changed;
    }
    root == plan.root_stamp
        && current.inventory().entries() == plan.entries
        && current.metadata().source_bytes() == plan.metadata
}
fn inspect(plan: &RemovalPlan, root: &Arc<Handle>, path: &Path, detached: bool) -> Result<()> {
    let current = inventory_retained(
        Arc::clone(root),
        path,
        plan.name.to_str().expect("validated metadata identity"),
        plan.limits.inventory,
    )
    .map_err(|e| {
        fail(
            path,
            "inspect complete Windows removal payload",
            RemovalFailureKind::Preflight(RemovalError::Discovery(DiscoveryError::Inventory(e))),
        )
    })?;
    if !same_payload(plan, &current, detached) {
        return Err(changed(path));
    }
    Ok(())
}
fn restore(
    plan: &RemovalPlan,
    stage: &Stage,
    root: &Arc<Handle>,
    report: &mut RemovalReport,
) -> Result<()> {
    stage.current(plan)?;
    binding(
        &stage.file,
        OsStr::new("payload"),
        &plan.root_stamp,
        Kind::Directory,
        &stage.path.join("payload"),
    )?;
    native::rename_exclusive(root, &stage.parent, &plan.name)
        .map_err(|e| io_error(&plan.path, "restore held payload without replacement", e))?;
    report.restored_before_removal = true;
    Ok(())
}
struct Directory {
    file: Arc<Handle>,
    expected: EntryStamp,
    security: Vec<u8>,
}
struct HeldFile {
    file: File,
    security: Vec<u8>,
}
struct Graph {
    dirs: BTreeMap<PathBuf, Directory>,
    files: BTreeMap<usize, HeldFile>,
}
impl Graph {
    fn build(plan: &RemovalPlan, stage: &Stage, root: &Arc<Handle>) -> Result<Self> {
        let label = stage.path.join("payload");
        let expected = stamp(root, &label)?;
        let security = security::descriptor(root)
            .map_err(|e| io_error(&label, "capture held root security", e))?;
        let mut graph = Self {
            dirs: BTreeMap::from([(
                PathBuf::new(),
                Directory {
                    file: Arc::clone(root),
                    expected,
                    security,
                },
            )]),
            files: BTreeMap::new(),
        };
        for entry in plan
            .entries
            .iter()
            .filter(|entry| entry.kind == SourceEntryKind::Directory)
        {
            let parent = &graph.dirs[entry.relative_path.parent().expect("validated parent")].file;
            let label = stage.path.join("payload").join(&entry.relative_path);
            let file = native::open_removal(
                parent,
                entry.relative_path.file_name().expect("validated name"),
                Kind::Directory,
            )
            .map_err(|e| io_error(&label, "pin approved directory for Windows removal", e))?;
            if stamp(&file, &label)? != entry.stamp {
                return Err(changed(&label));
            }
            let security = security::descriptor(&file)
                .map_err(|e| io_error(&label, "capture directory security", e))?;
            graph.dirs.insert(
                entry.relative_path.clone(),
                Directory {
                    file: Arc::new(Handle::new(file)),
                    expected: entry.stamp.clone(),
                    security,
                },
            );
        }
        for (index, entry) in plan
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.kind == SourceEntryKind::File)
        {
            let parent = &graph.dirs[entry.relative_path.parent().expect("validated parent")].file;
            let label = stage.path.join("payload").join(&entry.relative_path);
            let file = native::open_removal(
                parent,
                entry.relative_path.file_name().expect("validated name"),
                Kind::File,
            )
            .map_err(|e| io_error(&label, "pin approved file for Windows removal", e))?;
            if stamp(&file, &label)? != entry.stamp {
                return Err(changed(&label));
            }
            let security = security::descriptor(&file)
                .map_err(|e| io_error(&label, "capture file security", e))?;
            graph.files.insert(index, HeldFile { file, security });
        }
        Ok(graph)
    }
    fn into_root(mut self) -> Arc<Handle> {
        let root = self
            .dirs
            .remove(Path::new(""))
            .expect("retained selected root");
        self.files.clear();
        self.dirs.clear();
        root.file
    }
}
fn directory_current(relative: &Path, graph: &Graph, stage: &Stage) -> Result<()> {
    let directory = &graph.dirs[relative];
    let label = stage.path.join("payload").join(relative);
    let current = stamp(&directory.file, &label)?;
    // NTFS directory times may reflect this operation's prior child deletions
    // lazily. Identity/kind/readonly/security and exact held bindings remain gates.
    if current.file_identity() != directory.expected.file_identity()
        || current.mode != directory.expected.mode
        || security::descriptor(&directory.file)
            .map_err(|e| io_error(&label, "recheck directory security", e))?
            != directory.security
    {
        return Err(changed(&label));
    }
    if relative.as_os_str().is_empty() {
        binding(
            &stage.file,
            OsStr::new("payload"),
            &directory.expected,
            Kind::Directory,
            &label,
        )
    } else {
        let parent = relative.parent().expect("validated parent");
        directory_current(parent, graph, stage)?;
        binding(
            &graph.dirs[parent].file,
            relative.file_name().expect("validated name"),
            &directory.expected,
            Kind::Directory,
            &label,
        )
    }
}
fn audit_names(
    plan: &RemovalPlan,
    stage: &Stage,
    graph: &Graph,
    report: &mut RemovalReport,
) -> Result<()> {
    let mut expected: BTreeMap<PathBuf, BTreeSet<OsString>> = BTreeMap::new();
    for (index, entry) in plan.entries.iter().enumerate() {
        if report.outcomes[index].state != RemovalEntryState::Removed {
            expected
                .entry(
                    entry
                        .relative_path
                        .parent()
                        .expect("validated parent")
                        .to_owned(),
                )
                .or_default()
                .insert(
                    entry
                        .relative_path
                        .file_name()
                        .expect("validated name")
                        .to_owned(),
                );
        }
    }
    let mut names_left = plan.limits.inventory.max_entries;
    let mut bytes_left = 1024 * 1024usize;
    for (relative, directory) in &graph.dirs {
        directory_current(relative, graph, stage)?;
        let names = directory.file.entries(names_left).map_err(|e| {
            report.unrecognized_scan_incomplete = true;
            io_error(&stage.path, "bounded complete directory-name audit", e)
        })?;
        names_left -= names.len();
        let actual = names
            .into_iter()
            .map(|entry| entry.name)
            .collect::<BTreeSet<_>>();
        let wanted = expected.get(relative).cloned().unwrap_or_default();
        if actual != wanted {
            for name in actual.difference(&wanted) {
                let path = stage.path.join("payload").join(relative).join(name);
                let bytes = crate::planner::native::path_bytes(&path);
                if bytes > bytes_left {
                    report.unrecognized_scan_incomplete = true;
                    break;
                }
                bytes_left -= bytes;
                report.unrecognized_entries.push(path);
            }
            report.unrecognized_entries.sort();
            return Err(changed(&stage.path.join("payload").join(relative)));
        }
    }
    Ok(())
}
fn verify_file(file: &HeldFile, entry: &SourceEntry, path: &Path) -> Result<()> {
    if stamp(&file.file, path)? != entry.stamp
        || security::descriptor(&file.file)
            .map_err(|e| io_error(path, "recheck approved file security", e))?
            != file.security
    {
        return Err(changed(path));
    }
    let mut reader = &file.file;
    let mut read = 0u64;
    let mut hash = blake3::Hasher::new();
    let mut buffer = [0u8; 65536];
    loop {
        let remaining = entry
            .stamp
            .length
            .saturating_sub(read)
            .saturating_add(1)
            .min(buffer.len() as u64) as usize;
        let count = reader
            .read(&mut buffer[..remaining])
            .map_err(|e| io_error(path, "verify approved bytes before Windows disposition", e))?;
        if count == 0 {
            break;
        }
        read += count as u64;
        if read > entry.stamp.length {
            return Err(changed(path));
        }
        hash.update(&buffer[..count]);
    }
    if read != entry.stamp.length
        || entry.digest != Some(*hash.finalize().as_bytes())
        || stamp(&file.file, path)? != entry.stamp
    {
        return Err(changed(path));
    }
    Ok(())
}
fn into_file(handle: Arc<Handle>, path: &Path) -> Result<File> {
    Arc::try_unwrap(handle).map(Handle::into_file).map_err(|_| {
        fail(
            path,
            "close removal directory handles",
            RemovalFailureKind::Unsupported(
                "internal directory references remain; deletion refused",
            ),
        )
    })
}
fn remove_entries(
    plan: &RemovalPlan,
    stage: &Stage,
    graph: &mut Graph,
    report: &mut RemovalReport,
    run: &mut impl FnMut(Phase, &Path) -> io::Result<()>,
) -> Result<()> {
    let mut order = (0..plan.entries.len()).collect::<Vec<_>>();
    order.sort_by(|a, b| {
        plan.entries[*b]
            .relative_path
            .components()
            .count()
            .cmp(&plan.entries[*a].relative_path.components().count())
            .then_with(|| {
                plan.entries[*a]
                    .relative_path
                    .cmp(&plan.entries[*b].relative_path)
            })
    });
    let mut audited = false;
    for index in order {
        let entry = &plan.entries[index];
        let label = stage.path.join("payload").join(&entry.relative_path);
        let attempt = (|| {
            hook(Phase::BeforeUnlink, &label, run)?;
            stage.current(plan)?;
            if !audited {
                audit_names(plan, stage, graph, report)?;
                audited = true;
            }
            let parent = entry.relative_path.parent().expect("validated parent");
            directory_current(parent, graph, stage)?;
            let parent = Arc::clone(&graph.dirs[parent].file);
            let name = entry.relative_path.file_name().expect("validated name");
            let file = if entry.kind == SourceEntryKind::File {
                let file = graph.files.get(&index).expect("pinned approved file");
                verify_file(file, entry, &label)?;
                binding(&parent, name, &entry.stamp, Kind::File, &label)?;
                graph.files.remove(&index).expect("pinned file").file
            } else {
                directory_current(&entry.relative_path, graph, stage)?;
                if !graph.dirs[&entry.relative_path]
                    .file
                    .entries(0)
                    .map_err(|e| io_error(&label, "verify approved directory empty", e))?
                    .is_empty()
                {
                    return Err(changed(&label));
                }
                into_file(
                    graph
                        .dirs
                        .remove(&entry.relative_path)
                        .expect("pinned directory")
                        .file,
                    &label,
                )?
            };
            report.permanent_removal_attempted = true;
            native::dispose_verified(file)
                .map_err(|e| io_error(&label, "dispose approved held Windows object", e))?;
            removed_binding(&parent, name, &entry.stamp, kind(entry.kind), &label)?;
            report.outcomes[index].state = RemovalEntryState::Removed;
            hook(Phase::AfterUnlink, &label, run)
        })();
        if let Err(error) = attempt {
            if report.outcomes[index].state != RemovalEntryState::Removed {
                let parent = entry.relative_path.parent().expect("validated parent");
                let known = stage.current(plan).is_ok()
                    && directory_current(parent, graph, stage).is_ok()
                    && binding(
                        &graph.dirs[parent].file,
                        entry.relative_path.file_name().expect("validated name"),
                        &entry.stamp,
                        kind(entry.kind),
                        &label,
                    )
                    .is_ok();
                report.outcomes[index].state = if known {
                    RemovalEntryState::Remaining
                } else {
                    RemovalEntryState::Unknown
                };
            }
            report.outcomes[index].failure = Some((*error).clone());
            return Err(error);
        }
    }
    let label = stage.path.join("payload");
    let attempt = (|| {
        hook(Phase::BeforeUnlink, &label, run)?;
        stage.current(plan)?;
        directory_current(Path::new(""), graph, stage)?;
        graph.dirs[Path::new("")]
            .file
            .entries(0)
            .map_err(|e| io_error(&label, "verify selected root empty", e))?;
        let root = into_file(
            graph
                .dirs
                .remove(Path::new(""))
                .expect("root retained")
                .file,
            &label,
        )?;
        report.permanent_removal_attempted = true;
        native::dispose_verified(root)
            .map_err(|e| io_error(&label, "dispose verified empty held root", e))?;
        removed_binding(
            &stage.file,
            OsStr::new("payload"),
            &plan.root_stamp,
            Kind::Directory,
            &label,
        )?;
        report.outcomes.last_mut().expect("root outcome").state = RemovalEntryState::Removed;
        hook(Phase::AfterUnlink, &label, run)
    })();
    if let Err(error) = &attempt {
        let root = report.outcomes.last_mut().expect("root outcome");
        if root.state != RemovalEntryState::Removed {
            root.state = RemovalEntryState::Unknown;
        }
        root.failure = Some((**error).clone());
    }
    attempt
}
pub(super) fn apply(plan: &RemovalPlan, report: &mut RemovalReport) {
    apply_with_hook(plan, report, &mut |_, _| Ok(()));
}
fn apply_with_hook(
    plan: &RemovalPlan,
    report: &mut RemovalReport,
    run: &mut impl FnMut(Phase, &Path) -> io::Result<()>,
) {
    let prepared = super::super::plan::verify(plan)
        .map_err(|e| {
            fail(
                &plan.path,
                "Windows read-only removal preflight",
                RemovalFailureKind::Preflight(e),
            )
        })
        .and_then(|()| hook(Phase::BeforeRootOpen, &plan.path, run))
        .and_then(|()| {
            let file = native::open_removal(
                plan.namespace.retained_anchor().0,
                &plan.name,
                Kind::Directory,
            )
            .map_err(|e| {
                io_error(
                    &plan.path,
                    "open selected root with DELETE and read-only sharing",
                    e,
                )
            })?;
            let root = Arc::new(Handle::new(file));
            if stamp(&root, &plan.path)? != plan.root_stamp {
                return Err(changed(&plan.path));
            }
            let names = plan
                .namespace
                .retained_anchor()
                .0
                .entries(4096)
                .map_err(|e| io_error(&plan.path, "verify exact selected root spelling", e))?;
            if !names.iter().any(|entry| entry.name == plan.name) {
                return Err(changed(&plan.path));
            }
            inspect(plan, &root, &plan.path, false)?;
            Ok(root)
        });
    let root = match prepared {
        Ok(root) => root,
        Err(error) => {
            report.failure = Some(*error);
            return;
        }
    };
    if let Err(error) = hook(Phase::BeforeStage, &plan.path, run) {
        report.failure = Some(*error);
        return;
    }
    let stage = match create_stage(plan, report) {
        Ok(stage) => stage,
        Err(error) => {
            report.failure = Some(*error);
            return;
        }
    };
    let detach = hook(Phase::AfterStage, &stage.path, run)
        .and_then(|()| stage.current(plan))
        .and_then(|()| hook(Phase::BeforeDetach, &plan.path, run))
        .and_then(|()| {
            if stamp(&root, &plan.path)? != plan.root_stamp {
                return Err(changed(&plan.path));
            }
            native::rename_exclusive(&root, &stage.file, OsStr::new("payload"))
                .map_err(|e| io_error(&plan.path, "native held-object no-replace detach", e))
        });
    if let Err(error) = detach {
        report.failure = Some(*error);
        drop(root);
        stage.cleanup(plan, report, run);
        return;
    }
    let prepared = hook(Phase::AfterDetach, &stage.path.join("payload"), run)
        .and_then(|()| stage.current(plan))
        .and_then(|()| inspect(plan, &root, &stage.path.join("payload"), true))
        .and_then(|()| Graph::build(plan, &stage, &root));
    let mut graph = match prepared {
        Ok(graph) => graph,
        Err(error) => {
            report.failure = Some(*error);
            report.unrecognized_scan_incomplete = true;
            match restore(plan, &stage, &root, report) {
                Ok(()) => {
                    drop(root);
                    stage.cleanup(plan, report, run);
                }
                Err(error) => {
                    report.stage_location_uncertain |= stage.current(plan).is_err();
                    report.cleanup_failures.push(*error);
                }
            }
            return;
        }
    };
    drop(root);
    match remove_entries(plan, &stage, &mut graph, report, run) {
        Ok(()) => {
            drop(graph);
            stage.cleanup(plan, report, run);
        }
        Err(error) => {
            report.failure = Some(*error);
            if !report.permanent_removal_attempted {
                let root = graph.into_root();
                match restore(plan, &stage, &root, report) {
                    Ok(()) => {
                        drop(root);
                        stage.cleanup(plan, report, run);
                    }
                    Err(error) => {
                        report.stage_location_uncertain |= stage.current(plan).is_err();
                        report.cleanup_failures.push(*error);
                    }
                }
            } else {
                drop(graph);
                if stage.current(plan).is_err() {
                    report.stage_location_uncertain = true;
                }
                report.unrecognized_scan_incomplete = true;
            }
        }
    }
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
