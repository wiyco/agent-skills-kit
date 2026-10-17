# Architecture and safety boundaries

## Product contract

A caller supplies a local skill directory, selected target IDs, an explicit installation scope, and a context describing the user's environment. The library validates the skill, resolves documented destinations, produces a read-only plan, and applies an approved copy. The CLI is one consumer, not the owner of this behavior.

Use one library crate (`agent-skills-kit`) and one CLI crate (`agent-skills-kit-cli`); split out the catalog only if a real API/dependency benefit appears. Naming and licensing decisions are recorded in [ADR 0005](adr/0005-naming-licensing-and-reuse.md).

```text
CLI / another Rust application
              |
       public library facade
              |
 catalog -> resolver -> validation/inventory -> planner -> executor
              |                                |           |
       explicit context                   InstallPlan  InstallReport
```

## Boundaries

**Catalog.** Owns target IDs, product variants, path rules, capabilities, evidence, and known constraints. It distinguishes a preferred write destination from all locations the host can read. The catalog is bundled and versioned, not silently downloaded at runtime.

**Resolver.** Computes paths from explicit caller context. It does not create directories, mutate environment variables, or infer an undocumented XDG/home convention. A project root and an application workspace root are different concepts.

**Validator/inventory.** Reads SKILL.md and bounded supporting resources as inert data. General specification validation and host-specific validation are separate. [Agent Skills specification](https://agentskills.io/specification).

**Planner.** Reads the filesystem to identify changes, unchanged content, conflicts, unsupported combinations, shared-location effects, and preconditions. It performs no writes, including lockfiles or temporary directories. Output is deterministic.

**Executor.** Applies only a reviewed plan after revalidating preconditions. It owns staging, copy, no-clobber commit behavior, cleanup, and per-destination outcomes. It never executes skill content.

**CLI.** Owns arguments, terminal interaction, confirmation, process exit codes, and rendering. It cannot bypass library safety checks.

## Proposed public concepts

These are design concepts, not claims about an existing Rust API:

- `TargetId`, `TargetDefinition`, `TargetRegistry`, `PathRule`, `EvidenceRecord`.
- `InstallContext`, `Scope`, `ResolvedDestination`, `ResolutionWarning`.
- `ValidatedSkill`, `SourceInventory`, `InstallRequest`, `InstallPlan`.
- `InstallReport`, `DestinationOutcome`, `InstallReceipt`, typed errors.

Use `PathBuf`/`OsString` for filesystem context. Avoid assumptions that paths are UTF-8. Serde report encoding should represent non-UTF-8 values explicitly rather than panicking or silently losing identity. Prefer a synchronous local-filesystem API unless actual requirements justify an async interface.

## Scope and placement

`Project` means a caller-specified root. `User` means a caller-specified/discovered user environment with adapter-specific rules. Other workspace/admin/system scopes must be explicit capabilities; the MVP does not write to managed or bundled stores.

A selected tool chooses an installation destination, not exclusive access. For example, OpenCode documents reading its own paths plus Claude-compatible and agent-compatible paths; a skill installed for another host may therefore be discoverable by OpenCode. [OpenCode skills](https://opencode.ai/docs/skills/).

Do not silently replace native placements with a single shared placement as an optimization. Deduplicate only genuinely identical resolved destinations and preserve all target attribution in the plan. A future explicit shared-placement policy must disclose its implications.

## Evidence and support model

Each rule stores the host-owned documentation URL, relevant section, access date, applicable product version, OS/scope, and any uncertainty. Maintain three independent facts: documentation reviewed, resolver tested, and actual host discovery tested. None implies the others.

A catalog candidate can be `unverified`, `documented`, `configuration-required`, `import-only`, or `unsupported`. Advertised installability is limited to documented and tested combinations. A CLI/IDE/cloud variant is a separate record when its behavior differs.

## MVP safety policy

Default to copying regular files and directories only. Reject untrusted descendant symlinks, reparse points, special files, traversal/absolute path injection, unsafe names, source/destination overlap, and conflicting existing content. Root canonicalization is an explicit trust decision, not permission to follow every descendant link.

The exact no-follow/no-replace backend is decided in ASK-008 and tested in ASK-012. Checking that a path is absent and then doing an ordinary rename is not a race-free no-clobber implementation. Fail closed when the selected backend cannot meet the promised guarantee.

The [reviewed threat model](threat-model.md) makes caller/root trust, supported concurrency, platform capability gates, cleanup ownership, and unsupported adversarial cases explicit. [ADR 0006](adr/0006-native-copy-safety.md) records the maintainer's contract review before executor work; neither document is proof of a working backend.

Stage on the destination filesystem, verify copied data, and apply using the reviewed no-replace strategy. Revalidate source and destination preconditions. Handle concurrent changes and interruptions without deleting unrelated data. Strip privilege-bearing file bits; preserve only documented ordinary file/executable intent.

A multi-target operation is not globally atomic. Return an outcome for every target and report partial success. Never remove a previously successful install in another root merely to imitate a global rollback.

Identical content may be unchanged after validation/comparison. Different existing content is a conflict. `--yes` is confirmation, not `--force`. Destructive replace, update, and uninstall are separate post-MVP operations.

Receipts are returned to the caller. Persistent receipt storage is explicit, schema-versioned, and outside the skill payload. No hidden home-directory state is introduced accidentally.

## Trust limitations

Safe copying is not sandboxing and metadata validation is not malware review. A later host may execute scripts or instructions from a skill. The installer must never suggest that installation establishes the skill's trustworthiness.

No telemetry, network acquisition, credential handling, or model-provider access is required for the MVP. Optional real-host tests are isolated and require appropriate user authorization.

## Non-goals for v0.1

Remote Git/URL/archive acquisition, symbolic-link installation, automatic marketplace integration, cloud account import, destructive replace, managed updates/uninstall, and a universal guarantee that all AI products are supported.

The experimental host-first manager reuses catalog/resolver/inventory APIs for bounded existing-skill discovery and the same install planner/executor for cross-agent copies. A validated exact source/destination identity with an already-proven `Unchanged` action is permitted as a no-write observation; containing/nested overlaps or intended writes to a source remain forbidden. This narrow exception supports shared-location/no-op selections without creating replacement authority. Discovery itself grants no ownership or deletion authority; see [discovery](discovery.md) and [CLI management](cli.md#experimental-host-first-management).

The separately authorized permanent-removal operation follows [ADR 0009](adr/0009-permanent-removal.md). It requires its own library plan/approval/report and contents/shared-reader preview, with native implementations and evidence separate from installation cleanup. A CLI prompt or receipt cannot expand an install plan into deletion authority. The ADR is an implementation contract, not a claim that removal is already available.
