# Existing-skill discovery

The read-only `discovery::list_skills` API observes skills for one catalog target, explicit scope and `InstallContext`. It is the library foundation for the host-first CLI manager tracked in #112. It adds no CLI command, copy engine, deletion permission, environment discovery or network access.

```rust
use agent_skills_kit::{
    catalog::{Catalog, Scope},
    discovery::{DiscoveryLimits, list_skills},
    resolver::{InstallContext, native_platform},
};
# fn inspect_explicit_home(home: std::path::PathBuf) -> Result<(), Box<dyn std::error::Error>> {
let catalog = Catalog::bundled()?;
let mut context = InstallContext::new(native_platform().ok_or("unsupported platform")?);
context.user_home = Some(home); // Caller-selected root, not an implicit HOME read.
let listing = list_skills(&catalog, "codex", Scope::User, &context, DiscoveryLimits::default())?;
for location in &listing.locations {
    // Inspect location.problem and every skill.inspection before presenting choices.
    // A compatibility reader is not permission to write/remove its contents.
    let _ = (&location.path, location.preferred_write, &location.skills);
}
# Ok(())
# }
```

Only catalog-resolved roots are considered. Native write and documented reader roles are retained separately, exact duplicate labels are merged, and an already observed physical root is marked as an alias rather than scanned twice. Ordering uses native path/name ordering; names are never lossy-converted or rewritten. Ancestor-to-repository traversal is returned in `unexpanded_ancestors`, not expanded outside explicitly selected roots or advertised as an exhaustive host discovery list. Actual host loading remains separate evidence.

The explicit context/override root is the trust boundary. Host-specific path components beneath it are walked through the existing retained no-follow namespace backend; a link in an intermediate `.agents`/other host component is rejected. Listing must not silently promote the derived skills directory's ancestors to caller-trusted roots. Candidate payloads are opened relative to the retained container, inspected through the existing inert inventory walker and checked against their current native binding. A changed/replaced root or changed name set invalidates the location. No shell/skill instructions are executed.

A missing verified root has `missing=true`, no problem and no entries. Unsafe, unreadable, sharing-denied and I/O failures have an explicit location problem, never an empty-success claim. Existing non-skill/invalid entries remain rows with failed inspection. Case/canonical-fold collisions use the planner's existing conservative native-name keys and reject both entries; they do not emulate every filesystem's collation. Opaque candidate names remain native values and fail general metadata identity rather than becoming replacement characters. On measured macOS/APFS, such a basename cannot be created (EILSEQ); that is a native capability refusal, not passing opaque-name discovery coverage.

Defaults/hard ceilings: 16/64 distinct location labels, 256/4096 immediate candidates, 16384/65536 payload entries, 256 MiB/1 GiB regular-file read bytes, plus the existing per-inventory limits. Directory enumeration may inspect one extra name to detect excess; the name set is re-enumerated for mutation detection. All names count, including rejected entries. Failed inventory attempts conservatively consume their full allocated entry/byte allowance because the fail-fast inventory API cannot report partial consumption. Later entries remain explicit budget failures. A location-count failure is fatal; other location/entry failures preserve bounded observations and never imply completeness.

These are observations, not an atomic multi-root snapshot. Source changes after listing do not become approved new contents: pass the selected `ValidatedSkill` through the existing fresh install planner, which checks its complete identity/bytes again. Later permanent-removal planning has a separately reviewed contract in #113; no receipt or listing row grants ownership/removal authority. Caller root/ancestor control, native filesystem eligibility and unsupported adversarial conditions remain those of the accepted threat model.

Reproduction: `cargo test -p agent-skills-kit --test discovery --locked -- --nocapture` and `cargo test -p agent-skills-kit --lib discovery:: --locked -- --nocapture`, then the required full workspace/Clippy/docs matrix. Original owned fixtures cover core hosts/scopes, deterministic unchanged trees/bytes, missing roots, invalid/non-directory entries, bounds, unsafe host-component/skill links (actual Windows reparse fixture), native opaque-name behavior, permission/sharing refusal, stale selected-source rejection and a deterministic entry added after enumeration. Linux additionally creates a real case-collision pair. New native results belong to the final PR QA; no Windows/Linux result is inferred from local macOS execution.
