# Using the library independently

> **WIP — not ready for use.** This is development reference material, not a released or supported library/CLI. Exercise examples only in owned temporary fixtures; do not install into real skill stores. Public release and contribution/reporting readiness remain pending.

The `agent-skills-kit` library is the implementation boundary. Neither it nor the standalone consumer depends on `agent-skills-kit-cli`. No runtime network, prompt, process exit, global environment/configuration mutation or terminal renderer belongs in the library. Errors are returned; applications decide how to display them and whether to exit.

## Separate reusable operations

| Operation | Public API | Responsibility |
| --- | --- | --- |
| Registry | `Catalog::bundled`, `get`, `document`, `new`, `from_json` | Validated immutable definitions, aliases and evidence |
| Context and resolution | `InstallContext::new`, `native_platform`, `resolve`, `resolve_targets` | Explicit roots/platform/overrides, typed failures, no I/O |
| Input validation | `parse_metadata`, `inventory_skill` | Bounded inert metadata/resources and native snapshots |
| Preview | `plan_install`, `InstallPlan` accessors | Read-only destinations, conflicts, unchanged entries, warnings and intended writes |
| Preflight/apply | `verify_plan`, `apply_plan` | Revalidate an approved plan, native no-clobber copy and complete outcomes |
| Observational serialization | `Receipt::from_report`, `to_json`, `from_json` | Versioned bounded records; no ownership or cleanup authority |

An application chooses explicit roots and selected targets, validates sources, builds a plan, presents/checks all destinations and warnings, and only then calls `apply_plan` to approve that exact plan. `is_successful()` accounts for preflight, per-target failures and cleanup diagnostics. Preserve the full report even when receipt encoding/storage fails. `CommittedWithPermissionFailure` means data is installed with failed/partial permission finalization; do not treat it as an absent destination. Fresh unchanged retries never repair permissions or reclaim orphan ownership.

## Executable external example

[`examples/external-consumer`](../examples/external-consumer) is an independent unpublished binary package with its own workspace and committed lockfile, a path dependency on the library, and no CLI/dependency inheritance. It forbids unsafe code. Run from the repository root:

```sh
cargo run --manifest-path examples/external-consumer/Cargo.toml --locked
cargo test --manifest-path examples/external-consumer/Cargo.toml --locked
```

The program exclusively creates its own temporary root, supplies isolated home/project context, and removes that owned fixture at the end. It never installs into the real home, executes payloads or invokes a host/model service. It demonstrates resolution, native validation, create previews, explicit fixture-only approval, copying, alias deduplication, schema-1 receipt round trips, fresh unchanged results and preserved manual-edit conflicts. Tests repeat all three core targets in both scopes. Native CI runs it on Linux/macOS/Windows and on the declared MSRV, including actual Windows MSRV tests. A missing filesystem capability is a failure, not substituted string emulation.

The example's assertions make it a regression demonstration, not a production UI. Replace its owned-fixture setup and explicit approval with your application's caller-authorized context and review flow. Do not copy its fixture teardown into a general receipt/orphan cleanup mechanism.

## Extension without a fork

Export `Catalog::document()`, append an original `TargetDefinition`, and validate the resulting document with `Catalog::new`. IDs/aliases must stay unique; scopes, native rules and reviewed evidence must pass the same checks. No CLI modification, plugin runtime or library fork is required. User-provided metadata does not prove an official path or host discovery. Keep undocumented rules unverified; resolution refuses unreviewed/unsupported rules.

The external test registers a synthetic ID/alias reusing the already documented Codex placement unchanged. It verifies custom lookup/resolution/installation without advertising a new product, variant or target path. Unknown IDs and unsupported scopes return typed `ResolveError` variants. Future receipt versions fail with `ReceiptError::UnsupportedVersion` before interpreting their layout. Duplicate names, malformed rules and other negative catalog cases remain covered by the library's catalog tests.

## Compatibility review

The v0.1 API is synchronous and library-first; no async runtime, plugin framework or alternate installation engine is needed. Operation-owned handles/inventory/plans and validated catalog/receipt internals remain private. Public inputs are explicit; opaque native paths use `PathBuf`/`OsString`. Non-exhaustive public enums require downstream wildcard matches, allowing additive variants. Existing public constructible structs and their fields remain unchanged: adding required fields, tightening constructors or changing native identity semantics needs an explicit compatibility review/version change, even before 1.0. This issue does not silently add `non_exhaustive` to constructible structs.

Catalog schema 1 and receipt schema 1 are independent of the Rust package version. Strict parsing rejects unknown layouts/fields/enum values; wire changes need a deliberate new schema/migration, not reinterpretation. Receipts are observational and cannot be used as an executable plan. No serialization of retained filesystem capabilities is offered.

Native unsafe code remains confined to reviewed filesystem/permission modules; the facade and consumer add none. [ADR 0006](adr/0006-native-copy-safety.md), [execution](execution.md) and [Windows review](verification/windows-backend.md) record justification, native regressions and limitations. Self-review does not establish absolute correctness. Windows validation is Server 2025 x64/NTFS CI; unsupported filesystems, real host discovery and other security/architecture combinations remain outside demonstrated coverage.

## Package-content review

`cargo package -p agent-skills-kit --list --locked` lists the planned library archive without publishing. The catalog is compiled from `src/catalog/bundled.rs` and schema-validated embedded `src/catalog/data/expanded.json`; it does not depend on a repository-relative runtime file. Package listings must include both sources. The list contains the library source, original tests/fixtures, Cargo metadata/lockfile and Cargo-generated VCS metadata. It excludes the CLI, standalone consumer, global/local proto state, `.env`/credentials, `.git`, local build output and unrelated repository files. The consumer's lockfile independently records the same dependency graph.

Packaging metadata/license/readme completeness and an extracted-package build are further ASK-024 release rehearsals; this inspection is not publication approval. `publish = false` remains in both project packages and the external example. No registry login, upload, release or visibility change occurs.

Primary Cargo documentation reviewed 2026-10-16 UTC: [workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html), workspace membership/inheritance and lockfiles; [publishing](https://doc.rust-lang.org/cargo/reference/publishing.html), package-content inspection and publication permanence. These inform the isolated consumer/package checks, not authorization to publish.
