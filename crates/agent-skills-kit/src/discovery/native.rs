use super::*;
use crate::{
    planner::{
        DestinationPresence,
        native::{self, Directory},
        observe_destination,
    },
    validation::{EntryStamp, InventoryErrorKind, inventory_retained},
};
use std::{collections::BTreeMap, ffi::OsStr, sync::Arc};

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn stamp(directory: &Directory) -> io::Result<EntryStamp> {
    directory
        .metadata()
        .map(|metadata| crate::filesystem::stamp(&metadata))
}
#[cfg(windows)]
fn stamp(directory: &Directory) -> io::Result<EntryStamp> {
    crate::filesystem_windows::entry_stamp(directory)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn child(parent: &Directory, name: &OsStr) -> io::Result<Directory> {
    use rustix::fs::{Mode, OFlags};
    if !crate::filesystem::safe_component(name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe candidate name",
        ));
    }
    rustix::fs::openat(
        parent,
        name,
        crate::filesystem::flags() | OFlags::DIRECTORY,
        Mode::empty(),
    )
    .map(std::fs::File::from)
    .map_err(Into::into)
}
#[cfg(windows)]
fn child(parent: &Directory, name: &OsStr) -> io::Result<Directory> {
    let file = crate::filesystem_windows::child(
        parent,
        name,
        crate::filesystem_windows::Kind::Directory,
        false,
    )?;
    Ok(crate::filesystem_windows::reader::Handle::new(file))
}

pub(super) fn inspect_skill(
    snapshot: &DestinationSnapshot,
    name: &OsStr,
    limits: InventoryLimits,
) -> Result<Box<ValidatedSkill>, DiscoveryError> {
    let label = snapshot.destination().join(name);
    let text = name.to_str().ok_or_else(|| {
        DiscoveryError::Inventory(InventoryError {
            path: label.clone(),
            kind: InventoryErrorKind::NonUtf8SkillName,
        })
    })?;
    let root = child(snapshot.retained_anchor().0, name)
        .map_err(|e| DiscoveryError::io("open candidate no-follow", e))?;
    if stamp(&root)
        .map_err(|e| DiscoveryError::io("stat candidate", e))?
        .device
        != snapshot.retained_anchor().1.device
    {
        return Err(DiscoveryError::Inventory(InventoryError {
            path: label,
            kind: InventoryErrorKind::CrossDevice,
        }));
    }
    let inspected = inventory_retained(Arc::new(root), &label, text, limits)
        .map_err(DiscoveryError::Inventory)?;
    let binding = child(snapshot.retained_anchor().0, name)
        .map_err(|e| DiscoveryError::io("recheck candidate binding", e))?;
    if stamp(&binding).map_err(|e| DiscoveryError::io("stat candidate binding", e))?
        != *inspected.inventory().root_stamp()
    {
        return Err(DiscoveryError::Changed);
    }
    Ok(Box::new(inspected))
}

pub(super) fn inspect(listing: &mut SkillListing, limits: DiscoveryLimits) {
    inspect_with_hook(listing, limits, &mut |_| {});
}

fn inspect_with_hook(
    listing: &mut SkillListing,
    limits: DiscoveryLimits,
    before_candidate: &mut impl FnMut(&std::path::Path),
) {
    let (mut candidates, mut entries, mut bytes) = (0usize, 0usize, 0u64);
    let mut identities: BTreeMap<(u64, [u8; 16]), PathBuf> = BTreeMap::new();
    for location in &mut listing.locations {
        let result = (|| {
            let snapshot = observe_destination(&location.anchor, &location.relative)
                .map_err(DiscoveryError::Namespace)?;
            if snapshot.presence() == DestinationPresence::Missing {
                location.missing = true;
                if !snapshot.is_current().map_err(DiscoveryError::Namespace)? {
                    return Err(DiscoveryError::Changed);
                }
                location.snapshot = Some(snapshot);
                return Ok(());
            }
            let (directory, observed) = snapshot.retained_anchor();
            let before =
                stamp(directory).map_err(|e| DiscoveryError::io("stat listing root", e))?;
            if before != *observed {
                return Err(DiscoveryError::Changed);
            }
            if let Some(path) = identities.get(&before.file_identity()) {
                if !snapshot.is_current().map_err(DiscoveryError::Namespace)? {
                    return Err(DiscoveryError::Changed);
                }
                location.alias_of = Some(path.clone());
                location.snapshot = Some(snapshot);
                return Ok(());
            }
            let remaining = limits.max_candidates - candidates;
            if remaining == 0 {
                return Err(DiscoveryError::Limit("candidates"));
            }
            let mut names = native::names(directory, remaining)
                .map_err(|e| {
                    if e.kind() == io::ErrorKind::FileTooLarge {
                        DiscoveryError::Limit("candidates")
                    } else {
                        DiscoveryError::io("enumerate listing root", e)
                    }
                })?
                .take(remaining + 1)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| DiscoveryError::io("read listing entry", e))?;
            names.sort();
            if names.len() > remaining {
                candidates = limits.max_candidates;
                return Err(DiscoveryError::Limit("candidates"));
            }
            if names.windows(2).any(|pair| pair[0] == pair[1]) {
                return Err(DiscoveryError::Changed);
            }
            let ambiguous = crate::planner::ambiguous_names(&names);
            candidates += names.len();
            for name in names.iter().cloned() {
                before_candidate(&snapshot.destination().join(&name));
                let allocated_entries = limits
                    .inventory
                    .max_entries
                    .min(limits.max_entries - entries);
                let allocated_bytes = limits
                    .inventory
                    .max_total_bytes
                    .min(limits.max_read_bytes - bytes);
                let inspection = if ambiguous.contains(&name) {
                    Err(DiscoveryError::AmbiguousName)
                } else if allocated_entries == 0 || allocated_bytes == 0 {
                    Err(DiscoveryError::Limit("inventory entries/read bytes"))
                } else {
                    let mut per_skill = limits.inventory;
                    per_skill.max_entries = allocated_entries;
                    per_skill.max_total_bytes = allocated_bytes;
                    per_skill.max_file_bytes = per_skill.max_file_bytes.min(allocated_bytes);
                    let inspected = inspect_skill(&snapshot, &name, per_skill);
                    match &inspected {
                        Ok(skill) => {
                            entries += skill.inventory().entries().len();
                            bytes += skill.inventory().total_bytes();
                        }
                        Err(_) => {
                            entries += allocated_entries;
                            bytes += allocated_bytes;
                        }
                    }
                    inspected
                };
                location.skills.push(DiscoveredSkill {
                    path: snapshot.destination().join(&name),
                    name,
                    inspection,
                });
            }
            let mut after_names = native::names(directory, names.len())
                .map_err(|error| {
                    if error.kind() == io::ErrorKind::FileTooLarge {
                        DiscoveryError::Changed
                    } else {
                        DiscoveryError::io("recheck listing names", error)
                    }
                })?
                .take(names.len() + 1)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| DiscoveryError::io("recheck listing entry", error))?;
            after_names.sort();
            if after_names != names {
                return Err(DiscoveryError::Changed);
            }
            if stamp(directory).map_err(|e| DiscoveryError::io("recheck listing root", e))?
                != before
                || !snapshot.is_current().map_err(DiscoveryError::Namespace)?
            {
                return Err(DiscoveryError::Changed);
            }
            identities.insert(before.file_identity(), location.path.clone());
            location.snapshot = Some(snapshot);
            Ok(())
        })();
        if let Err(error) = result {
            location.problem = Some(error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Root(PathBuf);
    impl Drop for Root {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn entry_created_after_enumeration_invalidates_the_listing() {
        let path = std::env::temp_dir().join(format!(
            "ask-discovery-mutation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let fixture = Root(path);
        let home = fixture.0.join("home");
        let root = home.join(".agents/skills");
        fs::create_dir_all(root.join("original-skill")).unwrap();
        fs::write(
            root.join("original-skill/SKILL.md"),
            b"---\nname: original-skill\ndescription: Original inert fixture.\n---\n",
        )
        .unwrap();
        let mut context = InstallContext::new(crate::resolver::native_platform().unwrap());
        context.user_home = Some(home);
        let mut listing = list_skills(
            &Catalog::bundled().unwrap(),
            "codex",
            Scope::User,
            &context,
            DiscoveryLimits::default(),
        )
        .unwrap();
        for location in &mut listing.locations {
            location.snapshot = None;
            location.skills.clear();
            location.problem = None;
        }
        let mut ran = false;
        inspect_with_hook(&mut listing, DiscoveryLimits::default(), &mut |_| {
            assert!(!ran);
            ran = true;
            fs::create_dir(root.join("new-entry")).unwrap();
        });
        assert!(ran);
        assert!(
            listing
                .locations
                .iter()
                .any(|location| location.problem == Some(DiscoveryError::Changed))
        );
        assert!(root.join("new-entry").is_dir());
        drop(listing);
    }
}
