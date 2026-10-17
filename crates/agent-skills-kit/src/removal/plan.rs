use super::*;
use crate::{
    catalog::DiscoveryTraversal,
    discovery::{inspect_selected, observe_rule},
    receipt::NativePath,
    resolver::resolve,
};
use serde::Serialize;
use std::io::{self, Write};

fn same_payload(
    a: &crate::validation::ValidatedSkill,
    b: &crate::validation::ValidatedSkill,
) -> bool {
    a.metadata().source_bytes() == b.metadata().source_bytes()
        && a.inventory().root_stamp() == b.inventory().root_stamp()
        && a.inventory().entries() == b.inventory().entries()
}
fn identity(snapshot: &DestinationSnapshot) -> (u64, [u8; 16]) {
    snapshot.retained_anchor().1.file_identity()
}

pub(super) fn build(
    catalog: &Catalog,
    request: RemovalRequest,
) -> Result<RemovalPlan, RemovalError> {
    let limits = request.limits;
    if !limits.inventory.validate()
        || !(1..=1024).contains(&limits.max_targets)
        || !(1..=4096).contains(&limits.max_locations)
    {
        return Err(RemovalError::InvalidLimits);
    }
    if catalog.targets().len() > limits.max_targets {
        return Err(RemovalError::Limit("catalog targets"));
    }
    let listing = request.listing;
    let target_id = listing.target_id;
    let scope = listing.scope;
    let root = listing
        .locations
        .into_iter()
        .nth(request.location_index)
        .ok_or(RemovalError::InvalidSelection)?;
    if root.problem.is_some() || root.missing || root.alias_of.is_some() {
        return Err(RemovalError::InvalidSelection);
    }
    let namespace = root
        .snapshot()
        .cloned()
        .ok_or(RemovalError::InvalidSelection)?;
    if !namespace
        .is_current()
        .map_err(|e| RemovalError::Discovery(DiscoveryError::Namespace(e)))?
    {
        return Err(RemovalError::Changed);
    }
    let selected = root
        .skills
        .into_iter()
        .find(|row| row.name == request.skill_name)
        .ok_or(RemovalError::InvalidSelection)?
        .inspection
        .map_err(RemovalError::Discovery)?;
    let fresh = inspect_selected(&namespace, &request.skill_name, limits.inventory)
        .map_err(RemovalError::Discovery)?;
    if !same_payload(&selected, &fresh)
        || selected.inventory().requested_root()
            != namespace.destination().join(&request.skill_name)
    {
        return Err(RemovalError::Changed);
    }
    let mut plan = RemovalPlan {
        target_id,
        scope,
        path: namespace.destination().join(&request.skill_name),
        name: request.skill_name,
        root_stamp: fresh.inventory().root_stamp().clone(),
        entries: fresh.inventory().entries().to_vec(),
        metadata: fresh.metadata().source_bytes().to_vec(),
        bytes: fresh.inventory().total_bytes(),
        namespace,
        readers: vec![],
        unresolved: vec![],
        token: String::new(),
        limits,
    };
    protect_context_roots(&request.context, &plan)?;
    impact(catalog, &request.context, &mut plan)?;
    if !plan.namespace_is_current()? {
        return Err(RemovalError::Changed);
    }
    plan.token = fingerprint(catalog, &plan)?;
    // No payload handles survive in the plan. Only the container namespace is
    // retained; Windows can acquire the selected object with DELETE at apply.
    Ok(plan)
}

fn impact(
    catalog: &Catalog,
    context: &InstallContext,
    plan: &mut RemovalPlan,
) -> Result<(), RemovalError> {
    let wanted = identity(&plan.namespace);
    let mut source_matches = false;
    let mut observed = 0;
    for target in catalog.targets() {
        for placement in &target.scopes {
            let scope = placement.scope;
            let resolved = match resolve(catalog, &target.id, scope, context) {
                Ok(resolved) => resolved,
                Err(error) => {
                    plan.unresolved.push(UnresolvedRemovalReader {
                        target_id: target.id.clone(),
                        scope,
                        path: None,
                        reason: error.to_string(),
                    });
                    continue;
                }
            };
            let rules = std::iter::once((resolved.preferred_write, true, false)).chain(
                resolved.discovery.into_iter().map(|read| {
                    (
                        read.location,
                        false,
                        read.traversal != DiscoveryTraversal::Exact,
                    )
                }),
            );
            for (rule, preferred_write, ancestors_unexpanded) in rules {
                if observed == plan.limits.max_locations {
                    return Err(RemovalError::Limit("reader locations"));
                }
                observed += 1;
                let label = rule.path.clone();
                if ancestors_unexpanded {
                    plan.unresolved.push(UnresolvedRemovalReader { target_id:target.id.clone(), scope, path:Some(label.clone()), reason:"Documented ancestor search remains unexpanded; impact beyond this selected root is unknown.".into() });
                }
                let snapshot = match observe_rule(rule, context) {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        plan.unresolved.push(UnresolvedRemovalReader {
                            target_id: target.id.clone(),
                            scope,
                            path: Some(label),
                            reason: error.to_string(),
                        });
                        continue;
                    }
                };
                protect_snapshot(&snapshot, &label, plan.root_stamp.file_identity())?;
                if snapshot.presence() != DestinationPresence::ExistingDirectory {
                    continue;
                }
                if !snapshot
                    .is_current()
                    .map_err(|e| RemovalError::Discovery(DiscoveryError::Namespace(e)))?
                {
                    return Err(RemovalError::Changed);
                }
                if identity(&snapshot) != wanted {
                    continue;
                }
                if target.id == plan.target_id && scope == plan.scope {
                    source_matches = true;
                }
                if let Some(reader) = plan
                    .readers
                    .iter_mut()
                    .find(|r| r.target_id == target.id && r.scope == scope && r.path == label)
                {
                    reader.preferred_write |= preferred_write;
                    reader.ancestors_unexpanded |= ancestors_unexpanded;
                } else {
                    plan.readers.push(RemovalReader {
                        target_id: target.id.clone(),
                        scope,
                        path: label,
                        preferred_write,
                        ancestors_unexpanded,
                    });
                }
            }
        }
    }
    if !source_matches {
        return Err(RemovalError::ContextMismatch);
    }
    plan.readers
        .sort_by(|a, b| (&a.target_id, a.scope, &a.path).cmp(&(&b.target_id, b.scope, &b.path)));
    plan.unresolved.sort_by(|a, b| {
        (&a.target_id, a.scope, &a.path, &a.reason).cmp(&(
            &b.target_id,
            b.scope,
            &b.path,
            &b.reason,
        ))
    });
    plan.unresolved.dedup();
    Ok(())
}

struct Fingerprint {
    hash: blake3::Hasher,
    remaining: usize,
}
impl Write for Fingerprint {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(io::Error::other(
                "removal preview exceeds 8 MiB encoding budget",
            ));
        }
        self.remaining -= bytes.len();
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Fingerprint {
    fn value(&mut self, value: &impl Serialize) -> Result<(), RemovalError> {
        serde_json::to_writer(&mut *self, value)
            .map_err(|e| RemovalError::Encoding(e.to_string()))?;
        self.write_all(b"\n")
            .map_err(|e| RemovalError::Encoding(e.to_string()))
    }
    fn path(&mut self, path: &Path) -> Result<(), RemovalError> {
        self.value(
            &NativePath::from_native(path).map_err(|e| RemovalError::Encoding(e.to_string()))?,
        )
    }
    fn stamp(&mut self, stamp: &EntryStamp) -> Result<(), RemovalError> {
        self.value(&(
            stamp.file_identity(),
            stamp.mode,
            stamp.links,
            stamp.length,
            stamp.modified,
            stamp.changed,
        ))?;
        #[cfg(windows)]
        self.value(&(
            stamp.windows.attributes,
            stamp.windows.length,
            stamp.windows.modified,
            stamp.windows.changed,
        ))?;
        Ok(())
    }
}
fn fingerprint(catalog: &Catalog, plan: &RemovalPlan) -> Result<String, RemovalError> {
    let mut out = Fingerprint {
        hash: blake3::Hasher::new(),
        remaining: 8 * 1024 * 1024,
    };
    out.value(&(
        "agent-skills-kit/permanent-removal/v1",
        env!("CARGO_PKG_VERSION"),
    ))?;
    out.value(&catalog.document())?;
    out.value(&(&plan.target_id, plan.scope, plan.bytes))?;
    out.path(&plan.path)?;
    out.stamp(&plan.root_stamp)?;
    out.stamp(plan.namespace.retained_anchor().1)?;
    out.value(&plan.metadata)?;
    for entry in &plan.entries {
        out.path(&entry.relative_path)?;
        out.stamp(&entry.stamp)?;
        out.value(&(
            entry.kind == crate::validation::SourceEntryKind::Directory,
            entry.digest,
        ))?;
    }
    for reader in &plan.readers {
        out.value(&(
            &reader.target_id,
            reader.scope,
            reader.preferred_write,
            reader.ancestors_unexpanded,
        ))?;
        out.path(&reader.path)?;
    }
    for reader in &plan.unresolved {
        out.value(&(&reader.target_id, reader.scope, &reader.reason))?;
        out.value(
            &reader
                .path
                .as_deref()
                .map(NativePath::from_native)
                .transpose()
                .map_err(|e| RemovalError::Encoding(e.to_string()))?,
        )?;
    }
    Ok(out.hash.finalize().to_hex().to_string())
}

// Reject physical containment, including caller-controlled ancestor aliases and
// missing configured roots whose closest retained prefix is inside the payload.
fn protect_snapshot(
    snapshot: &DestinationSnapshot,
    label: &Path,
    payload: (u64, [u8; 16]),
) -> Result<(), RemovalError> {
    use crate::planner::native;
    let map_error = |error: std::io::Error| {
        RemovalError::Discovery(DiscoveryError::Namespace(crate::planner::NamespaceError {
            path: label.to_owned(),
            kind: crate::planner::NamespaceErrorKind::Io {
                operation: "inspect protected-root ancestry",
                kind: error.kind(),
                raw_os_error: error.raw_os_error(),
            },
        }))
    };
    let mut current = std::sync::Arc::clone(snapshot.retained_anchor().0);
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..128 {
        let identity = native::identity(&current).map_err(map_error)?;
        if identity == payload {
            return Err(RemovalError::ProtectedRoot(label.to_owned()));
        }
        if !seen.insert(identity) {
            return Err(RemovalError::Changed);
        }
        let parent = native::parent(&current).map_err(map_error)?;
        if native::identity(&parent).map_err(map_error)? == identity {
            return Ok(());
        }
        current = parent;
    }
    Err(RemovalError::Limit("protected-root ancestor depth"))
}
fn protect_context_roots(context: &InstallContext, plan: &RemovalPlan) -> Result<(), RemovalError> {
    for root in [
        &context.user_home,
        &context.project_root,
        &context.application_workspace,
    ]
    .into_iter()
    .flatten()
    {
        let snapshot = crate::planner::observe_destination(root, Path::new(""))
            .map_err(|error| RemovalError::Discovery(DiscoveryError::Namespace(error)))?;
        protect_snapshot(&snapshot, root, plan.root_stamp.file_identity())?;
    }
    Ok(())
}
