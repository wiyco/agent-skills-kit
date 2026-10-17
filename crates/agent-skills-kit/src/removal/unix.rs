use super::*;
use crate::{
    filesystem::{flags, stamp},
    validation::{SourceEntryKind, ValidatedSkill, inventory_retained},
};
use rustix::fs::{AtFlags, Mode, OFlags, RenameFlags};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs::File,
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

type Result<T> = std::result::Result<T, RemovalFailure>;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    BeforeStage,
    AfterStage,
    BeforeDetach,
    AfterDetach,
    BeforeUnlink,
    AfterUnlink,
    BeforeRestore,
    BeforeCleanup,
}
fn failure(path: &Path, operation: &'static str, kind: RemovalFailureKind) -> RemovalFailure {
    RemovalFailure {
        path: path.to_owned(),
        operation,
        kind,
    }
}
fn io_error(path: &Path, operation: &'static str, error: impl Into<io::Error>) -> RemovalFailure {
    let error = error.into();
    failure(
        path,
        operation,
        RemovalFailureKind::Io {
            kind: error.kind(),
            raw_os_error: error.raw_os_error(),
        },
    )
}
fn changed(path: &Path) -> RemovalFailure {
    failure(
        path,
        "verify retained identity/content/binding",
        RemovalFailureKind::Changed,
    )
}
fn metadata(file: &File, path: &Path) -> Result<EntryStamp> {
    file.metadata()
        .map(|m| stamp(&m))
        .map_err(|e| io_error(path, "stat retained removal object", e))
}
fn child(parent: &File, name: &OsStr, path: &Path, directory: bool) -> Result<Arc<File>> {
    if !crate::filesystem::safe_component(name) {
        return Err(changed(path));
    }
    rustix::fs::openat(
        parent,
        name,
        flags()
            | if directory {
                OFlags::DIRECTORY
            } else {
                OFlags::empty()
            },
        Mode::empty(),
    )
    .map(|fd| Arc::new(File::from(fd)))
    .map_err(|e| io_error(path, "open removal child no-follow", e))
}
fn bound(parent: &File, name: &OsStr, expected: &EntryStamp, path: &Path) -> Result<()> {
    let actual = rustix::fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|e| io_error(path, "check removal binding no-follow", e))?;
    if !crate::filesystem::identity_matches(&actual, expected) {
        return Err(changed(path));
    }
    Ok(())
}
fn hook(
    phase: Phase,
    path: &Path,
    run: &mut impl FnMut(Phase, &Path) -> io::Result<()>,
) -> Result<()> {
    run(phase, path).map_err(|e| io_error(path, "removal boundary hook", e))
}
struct Stage {
    parent: Arc<File>,
    file: Arc<File>,
    name: OsString,
    path: PathBuf,
    identity: EntryStamp,
}
impl Stage {
    fn current(&self, plan: &RemovalPlan) -> Result<()> {
        if !plan.namespace_is_current().map_err(|e| {
            failure(
                &self.path,
                "check removal container",
                RemovalFailureKind::Preflight(e),
            )
        })? {
            return Err(changed(&self.path));
        }
        let actual = metadata(&self.file, &self.path)?;
        if actual.file_identity() != self.identity.file_identity()
            || actual.mode & 0o7777 != 0o700
            || !crate::permissions_unix::no_acl(&self.file)
                .map_err(|e| io_error(&self.path, "check private removal ACL", e))?
        {
            return Err(changed(&self.path));
        }
        bound(&self.parent, &self.name, &actual, &self.path)
    }
    fn cleanup(
        &self,
        plan: &RemovalPlan,
        report: &mut RemovalReport,
        run: &mut impl FnMut(Phase, &Path) -> io::Result<()>,
    ) {
        let result = hook(Phase::BeforeCleanup, &self.path, run)
            .and_then(|()| self.current(plan))
            .and_then(|()| {
                rustix::fs::unlinkat(&self.parent, &self.name, AtFlags::REMOVEDIR)
                    .map_err(|e| io_error(&self.path, "remove verified empty private enclosure", e))
            });
        match result {
            Ok(()) => {
                report.retained_stage = None;
                report.stage_location_uncertain = false;
            }
            Err(error) => {
                report.retained_stage = Some(self.path.clone());
                report.stage_location_uncertain = true;
                report.cleanup_failures.push(error);
            }
        }
    }
}
fn stage(plan: &RemovalPlan, report: &mut RemovalReport) -> Result<Stage> {
    let parent = Arc::clone(plan.namespace.retained_anchor().0);
    #[cfg(target_os = "macos")]
    if rustix::fs::fstatfs(&parent)
        .map_err(|e| io_error(&plan.path, "inspect removal mount ownership", e))?
        .f_flags
        & libc::MNT_IGNORE_OWNERSHIP as u32
        != 0
    {
        return Err(failure(
            &plan.path,
            "private removal enclosure",
            RemovalFailureKind::Unsupported("filesystem ignores ownership"),
        ));
    }
    if !crate::permissions_unix::no_acl(&parent)
        .map_err(|e| io_error(&plan.path, "check removal creation-parent ACL", e))?
    {
        return Err(failure(
            &plan.path,
            "private removal enclosure",
            RemovalFailureKind::Unsupported("ACL-bearing creation parent"),
        ));
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    for _ in 0..100 {
        let name = OsString::from(format!(
            ".ask-remove-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let path = plan.namespace.destination().join(&name);
        match rustix::fs::mkdirat(&parent, &name, Mode::from_raw_mode(0o700)) {
            Ok(()) => (),
            Err(rustix::io::Errno::EXIST) => continue,
            Err(e) => {
                return Err(io_error(
                    &path,
                    "create exclusive private removal enclosure",
                    e,
                ));
            }
        }
        // Until identity/private mode are established, even a created name is
        // only an unconfirmed diagnostic, never automatic cleanup authority.
        report.retained_stage = Some(path.clone());
        report.stage_location_uncertain = true;
        let observed = rustix::fs::statat(&parent, &name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|e| io_error(&path, "inspect created removal enclosure", e))?;
        let file = child(&parent, &name, &path, true)?;
        let identity = metadata(&file, &path)?;
        if !crate::filesystem::binding_matches(&observed, &identity) {
            return Err(changed(&path));
        }
        let stage = Stage {
            parent,
            file,
            name,
            path,
            identity,
        };
        stage.current(plan)?;
        report.stage_location_uncertain = false;
        return Ok(stage);
    }
    Err(failure(
        &plan.path,
        "private removal enclosure",
        RemovalFailureKind::Unsupported("exclusive stage-name collision budget exhausted"),
    ))
}
#[allow(clippy::unnecessary_cast)]
fn stat_identity(stat: &rustix::fs::Stat) -> (u64, u64, u32) {
    (stat.st_dev as u64, stat.st_ino as u64, stat.st_mode as u32)
}
fn detach(
    plan: &RemovalPlan,
    stage: &Stage,
    did_detach: &mut bool,
    run: &mut impl FnMut(Phase, &Path) -> io::Result<()>,
) -> Result<(Arc<File>, EntryStamp)> {
    stage.current(plan)?;
    let fresh =
        crate::discovery::inspect_selected(&plan.namespace, &plan.name, plan.limits.inventory)
            .map_err(|e| {
                failure(
                    &plan.path,
                    "reinspect before detach",
                    RemovalFailureKind::Preflight(RemovalError::Discovery(e)),
                )
            })?;
    if fresh.inventory().root_stamp() != &plan.root_stamp
        || fresh.inventory().entries() != plan.entries
        || fresh.metadata().source_bytes() != plan.metadata
    {
        return Err(changed(&plan.path));
    }
    // The hook models the unavoidable Unix source-name gap. Verify the actual
    // detached object before any unlink; stat + rename is not inode-conditional.
    hook(Phase::BeforeDetach, &plan.path, run)?;
    rustix::fs::renameat_with(
        &stage.parent,
        &plan.name,
        &stage.file,
        "payload",
        RenameFlags::NOREPLACE,
    )
    .map_err(|e| io_error(&plan.path, "native no-replace private detach", e))?;
    *did_detach = true;
    let label = stage.path.join("payload");
    let file = child(&stage.file, OsStr::new("payload"), &label, true)?;
    let observed = metadata(&file, &label)?;
    Ok((file, observed))
}
fn matches_detached(plan: &RemovalPlan, current: &ValidatedSkill) -> bool {
    let mut root = current.inventory().root_stamp().clone();
    // Only the root metadata-change time is expected to change on detach.
    root.changed = plan.root_stamp.changed;
    root == plan.root_stamp
        && current.inventory().entries() == plan.entries
        && current.metadata().source_bytes() == plan.metadata
}
fn restore(
    plan: &RemovalPlan,
    stage: &Stage,
    detached: (u64, u64, u32),
    report: &mut RemovalReport,
    run: &mut impl FnMut(Phase, &Path) -> io::Result<()>,
) {
    let label = stage.path.join("payload");
    let result = hook(Phase::BeforeRestore, &label, run)
        .and_then(|()| stage.current(plan))
        .and_then(|()| {
            let current = rustix::fs::statat(&stage.file, "payload", AtFlags::SYMLINK_NOFOLLOW)
                .map_err(|e| io_error(&label, "verify detached binding before restoration", e))?;
            if stat_identity(&current) != detached {
                return Err(changed(&label));
            }
            rustix::fs::renameat_with(
                &stage.file,
                "payload",
                &stage.parent,
                &plan.name,
                RenameFlags::NOREPLACE,
            )
            .map_err(|e| io_error(&plan.path, "restore detached object without replacement", e))
        });
    match result {
        Ok(()) => {
            report.restored_before_removal = true;
            stage.cleanup(plan, report, run);
        }
        Err(error) => {
            if stage.current(plan).is_err() {
                report.stage_location_uncertain = true;
            }
            report.cleanup_failures.push(error);
        }
    }
}
struct Directory {
    file: Arc<File>,
    expected: EntryStamp,
}
fn directories(
    plan: &RemovalPlan,
    stage: &Stage,
    root: Arc<File>,
    stamp: EntryStamp,
) -> Result<BTreeMap<PathBuf, Directory>> {
    let mut result = BTreeMap::from([(
        PathBuf::new(),
        Directory {
            file: root,
            expected: stamp,
        },
    )]);
    for entry in plan
        .entries
        .iter()
        .filter(|e| e.kind == SourceEntryKind::Directory)
    {
        let label = stage.path.join("payload").join(&entry.relative_path);
        let parent = result
            .get(
                entry
                    .relative_path
                    .parent()
                    .expect("validated relative entry"),
            )
            .ok_or_else(|| changed(&label))?;
        let name = entry
            .relative_path
            .file_name()
            .expect("validated component");
        let file = child(&parent.file, name, &label, true)?;
        if metadata(&file, &label)? != entry.stamp {
            return Err(changed(&label));
        }
        bound(&parent.file, name, &entry.stamp, &label)?;
        result.insert(
            entry.relative_path.clone(),
            Directory {
                file,
                expected: entry.stamp.clone(),
            },
        );
    }
    Ok(result)
}
fn directory_current(
    relative: &Path,
    dirs: &BTreeMap<PathBuf, Directory>,
    stage: &Stage,
) -> Result<()> {
    let current = dirs
        .get(relative)
        .expect("complete retained directory graph");
    let label = stage.path.join("payload").join(relative);
    if metadata(&current.file, &label)? != current.expected {
        return Err(changed(&label));
    }
    if relative.as_os_str().is_empty() {
        bound(
            &stage.file,
            OsStr::new("payload"),
            &current.expected,
            &label,
        )?;
    } else {
        let parent = relative.parent().expect("validated relative directory");
        directory_current(parent, dirs, stage)?;
        bound(
            &dirs[parent].file,
            relative.file_name().expect("validated name"),
            &current.expected,
            &label,
        )?;
    }
    Ok(())
}
fn file_current(parent: &File, name: &OsStr, entry: &SourceEntry, label: &Path) -> Result<()> {
    let file = child(parent, name, label, false)?;
    if metadata(&file, label)? != entry.stamp {
        return Err(changed(label));
    }
    let mut reader = &*file;
    let mut read = 0u64;
    let mut digest = blake3::Hasher::new();
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
            .map_err(|e| io_error(label, "recheck approved file bytes before unlink", e))?;
        if count == 0 {
            break;
        }
        read += count as u64;
        if read > entry.stamp.length {
            return Err(changed(label));
        }
        digest.update(&buffer[..count]);
    }
    if read != entry.stamp.length
        || entry.digest != Some(*digest.finalize().as_bytes())
        || metadata(&file, label)? != entry.stamp
    {
        return Err(changed(label));
    }
    bound(parent, name, &entry.stamp, label)
}
fn remove_entries(
    plan: &RemovalPlan,
    stage: &Stage,
    mut dirs: BTreeMap<PathBuf, Directory>,
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
    for index in order {
        let entry = &plan.entries[index];
        let label = stage.path.join("payload").join(&entry.relative_path);
        let attempt = (|| {
            hook(Phase::BeforeUnlink, &label, run)?;
            stage.current(plan)?;
            let parent = entry
                .relative_path
                .parent()
                .expect("validated relative parent");
            directory_current(parent, &dirs, stage)?;
            let name = entry
                .relative_path
                .file_name()
                .expect("validated component");
            if entry.kind == SourceEntryKind::Directory {
                directory_current(&entry.relative_path, &dirs, stage)?;
            } else {
                file_current(&dirs[parent].file, name, entry, &label)?;
            }
            let flags = if entry.kind == SourceEntryKind::Directory {
                AtFlags::REMOVEDIR
            } else {
                AtFlags::empty()
            };
            rustix::fs::unlinkat(&dirs[parent].file, name, flags).map_err(|e| {
                io_error(&label, "unlink approved entry inside private enclosure", e)
            })?;
            report.outcomes[index].state = RemovalEntryState::Removed;
            let directory = dirs.get_mut(parent).expect("retained parent");
            let updated = metadata(&directory.file, &label)?;
            if updated.file_identity() != directory.expected.file_identity()
                || updated.mode != directory.expected.mode
            {
                return Err(changed(&label));
            }
            directory.expected = updated;
            hook(Phase::AfterUnlink, &label, run)
        })();
        if let Err(error) = attempt {
            if report.outcomes[index].state != RemovalEntryState::Removed {
                let parent = entry.relative_path.parent().expect("validated parent");
                let known = stage.current(plan).is_ok()
                    && directory_current(parent, &dirs, stage).is_ok()
                    && bound(
                        &dirs[parent].file,
                        entry.relative_path.file_name().expect("validated name"),
                        &entry.stamp,
                        &label,
                    )
                    .is_ok();
                report.outcomes[index].state = if known {
                    RemovalEntryState::Remaining
                } else {
                    RemovalEntryState::Unknown
                };
            }
            report.outcomes[index].failure = Some(error.clone());
            return Err(error);
        }
    }
    let label = stage.path.join("payload");
    let attempt = (|| {
        hook(Phase::BeforeUnlink, &label, run)?;
        stage.current(plan)?;
        directory_current(Path::new(""), &dirs, stage)?;
        rustix::fs::unlinkat(&stage.file, "payload", AtFlags::REMOVEDIR)
            .map_err(|e| io_error(&label, "remove verified empty selected root", e))?;
        report.outcomes.last_mut().expect("root outcome").state = RemovalEntryState::Removed;
        hook(Phase::AfterUnlink, &label, run)
    })();
    if let Err(error) = &attempt {
        let root = report.outcomes.last_mut().expect("root outcome");
        if root.state != RemovalEntryState::Removed {
            root.state = RemovalEntryState::Unknown;
        }
        root.failure = Some(error.clone());
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
    let preparation = hook(Phase::BeforeStage, &plan.path, run).and_then(|()| {
        super::super::plan::verify(plan).map_err(|e| {
            failure(
                &plan.path,
                "revalidate before staging",
                RemovalFailureKind::Preflight(e),
            )
        })
    });
    if let Err(error) = preparation {
        report.failure = Some(error);
        return;
    }
    let stage = match stage(plan, report) {
        Ok(stage) => stage,
        Err(error) => {
            report.failure = Some(error);
            return;
        }
    };
    if let Err(error) = hook(Phase::AfterStage, &stage.path, run) {
        report.failure = Some(error);
        stage.cleanup(plan, report, run);
        return;
    }
    let mut did_detach = false;
    let detached = detach(plan, &stage, &mut did_detach, run);
    if !did_detach {
        if let Err(error) = detached {
            report.failure = Some(error);
        }
        stage.cleanup(plan, report, run);
        return;
    }
    // Always inspect the actual private name after detach, including failure to
    // open a substituted link/type. Never unlink an unverified detached object.
    let actual = rustix::fs::statat(&stage.file, "payload", AtFlags::SYMLINK_NOFOLLOW);
    let identity = match actual {
        Ok(stat) => Some(stat_identity(&stat)),
        Err(rustix::io::Errno::NOENT) => None,
        Err(error) => {
            report.failure = Some(io_error(
                &stage.path,
                "identify actual private payload",
                error,
            ));
            return;
        }
    };
    let (root, root_stamp) = match detached {
        Ok(value) => value,
        Err(error) => {
            report.failure = Some(error);
            if let Some(identity) = identity {
                restore(plan, &stage, identity, report, run);
            } else {
                stage.cleanup(plan, report, run);
            }
            return;
        }
    };
    let verified = (|| {
        hook(Phase::AfterDetach, &stage.path.join("payload"), run)?;
        stage.current(plan)?;
        let current = inventory_retained(
            Arc::clone(&root),
            &stage.path.join("payload"),
            plan.name
                .to_str()
                .expect("validated UTF-8 metadata identity"),
            plan.limits.inventory,
        )
        .map_err(|e| {
            failure(
                &stage.path,
                "verify complete detached payload",
                RemovalFailureKind::Preflight(RemovalError::Discovery(DiscoveryError::Inventory(
                    e,
                ))),
            )
        })?;
        if !matches_detached(plan, &current) {
            return Err(changed(&stage.path));
        }
        let current_stamp = metadata(&root, &stage.path.join("payload"))?;
        if current_stamp != root_stamp {
            return Err(changed(&stage.path));
        }
        bound(
            &stage.file,
            OsStr::new("payload"),
            &current_stamp,
            &stage.path,
        )?;
        directories(plan, &stage, root, current_stamp)
    })();
    let dirs = match verified {
        Ok(dirs) => dirs,
        Err(error) => {
            report.failure = Some(error);
            if let Some(identity) = identity {
                restore(plan, &stage, identity, report, run);
            }
            return;
        }
    };
    match remove_entries(plan, &stage, dirs, report, run) {
        Ok(()) => stage.cleanup(plan, report, run),
        Err(error) => {
            report.failure = Some(error);
            if stage.current(plan).is_err() {
                report.stage_location_uncertain = true;
            }
        }
    }
}

#[cfg(test)]
#[path = "unix_tests.rs"]
mod tests;
