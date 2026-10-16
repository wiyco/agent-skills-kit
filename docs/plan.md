# Implementation plan

## Objective

Build and eventually publish a reusable Rust library that installs local Agent Skills into officially documented directories for selected tools. Build a thin CLI on top. Use English throughout. The maintainer controls source visibility separately from release-candidate completion, crate publication and binary distribution; see [source visibility readiness](public-readiness.md). A public development repository must describe incomplete work accurately.

## User journey

Choose a local skill -> choose one or more tools -> choose project/user scope -> inspect exact destinations and conflicts -> approve a safe copy -> inspect per-target results. Tool selection means choosing the host's documented placement, not typing arbitrary output paths. Advanced custom targets may extend the registry.

## v0.1 boundary

The library must work independently of the CLI. Required capabilities: explicit context resolution, validated local input, read-only planning, safe copy/no-clobber behavior, conflict/unchanged reporting, bounded filesystem handling, native tests, and consumer documentation. Codex, Claude Code, and OpenCode are the first evidence-backed targets.

Catalog expansion is parallel work: maximize documented/tested coverage, not an arbitrary advertised count. Unsupported or unverified products are legitimate research outcomes. Installation via web UI/import, remote acquisition, links, update, and uninstall are not hidden requirements of the copy MVP.

## Milestones

| Milestone | Outcome | Gate |
| --- | --- | --- |
| M0 - Foundation | Cargo workspace, proto-compatible pin, CI, OSS decisions, threat model | Reproducible and reviewed basis |
| M1 - Library MVP | Catalog/resolver/validator/planner/executor/API | Core targets and safety tests pass |
| M2 - CLI MVP | Scriptable CLI and optional prompts | CLI uses the same library API |
| M3 - Verified catalog expansion | Additional verified adapters and discovery process | Evidence per shipped rule; no guessed paths |
| M4 - OSS and release readiness | Security, packaging, binary rehearsals, release gate | Explicit maintainer publication approvals |
| M5 - Post-MVP | Links, acquisition, update/uninstall | Separate approved scope and threat review |

## Task catalog

`ASK-###` is a stable planning key, not a GitHub issue number. The importer resolves dependency keys to actual issue links. Each linked task includes scope, acceptance criteria, tests, non-goals, sources, and handoff instructions.

| Key | Task | Milestone | Priority | Depends on |
| --- | --- | --- | --- | --- |
| [ASK-001](../planning/issues/ASK-001.md) | Bootstrap the Cargo workspace and proto-compatible toolchain | M0 | P0 | None |
| [ASK-002](../planning/issues/ASK-002.md) | Finalize project naming, licensing, and contributor conventions | M0 | P0 | None |
| [ASK-003](../planning/issues/ASK-003.md) | Establish native OS CI and a measured MSRV policy | M0 | P0 | ASK-001 |
| [ASK-004](../planning/issues/ASK-004.md) | Specify a versioned target catalog and evidence model | M1 | P0 | ASK-001 |
| [ASK-005](../planning/issues/ASK-005.md) | Implement deterministic scope and environment resolution | M1 | P0 | ASK-004 |
| [ASK-006](../planning/issues/ASK-006.md) | Verify and implement Codex, Claude Code, and OpenCode targets | M1 | P0 | ASK-005, ASK-008 |
| [ASK-007](../planning/issues/ASK-007.md) | Implement skill validation and bounded source inventory | M1 | P0 | ASK-001, ASK-008 |
| [ASK-008](../planning/issues/ASK-008.md) | Define the filesystem threat model and copy safety contract | M0 | P0 | None |
| [ASK-009](../planning/issues/ASK-009.md) | Implement read-only install plans and destination deduplication | M1 | P0 | ASK-005, ASK-006, ASK-007, ASK-008 |
| [ASK-010](../planning/issues/ASK-010.md) | Implement staged copy execution with no-clobber defaults | M1 | P0 | ASK-009, ASK-008 |
| [ASK-011](../planning/issues/ASK-011.md) | Add idempotency, installation receipts, and recovery diagnostics | M1 | P1 | ASK-010 |
| [ASK-012](../planning/issues/ASK-012.md) | Build native filesystem and security regression coverage | M1 | P0 | ASK-003, ASK-010, ASK-011 |
| [ASK-013](../planning/issues/ASK-013.md) | Stabilize the reusable API and add external consumer examples | M1 | P0 | ASK-006, ASK-009, ASK-010, ASK-011, ASK-012 |
| [ASK-014](../planning/issues/ASK-014.md) | Implement the non-interactive CLI using the library | M2 | P0 | ASK-013 |
| [ASK-015](../planning/issues/ASK-015.md) | Add interactive target and scope selection | M2 | P1 | ASK-014 |
| [ASK-016](../planning/issues/ASK-016.md) | Verify Cursor, Gemini CLI, and GitHub Copilot variants | M3 | P1 | ASK-004, ASK-005, ASK-006 |
| [ASK-017](../planning/issues/ASK-017.md) | Verify Cline, Roo Code, and Kilo Code variants | M3 | P1 | ASK-004, ASK-005, ASK-006 |
| [ASK-018](../planning/issues/ASK-018.md) | Verify Junie, Factory Droid, Kimi Code, and Qwen Code | M3 | P1 | ASK-004, ASK-005, ASK-006 |
| [ASK-019](../planning/issues/ASK-019.md) | Verify Mistral Vibe, Amp, Pi, and Crush | M3 | P1 | ASK-004, ASK-005, ASK-006 |
| [ASK-020](../planning/issues/ASK-020.md) | Verify workspace-based hosts and renamed product variants | M3 | P1 | ASK-004, ASK-005, ASK-006 |
| [ASK-021](../planning/issues/ASK-021.md) | Add ongoing host discovery and catalog drift review | M3 | P1 | ASK-004, ASK-006 |
| [ASK-022](../planning/issues/ASK-022.md) | Add opt-in real-host discovery smoke tests | M3 | P1 | ASK-012, ASK-014 |
| [ASK-023](../planning/issues/ASK-023.md) | Add dependency, license, and CI supply-chain checks | M4 | P0 | ASK-002, ASK-003 |
| [ASK-024](../planning/issues/ASK-024.md) | Rehearse the library release and document publishing controls | M4 | P0 | ASK-002, ASK-003, ASK-013, ASK-023 |
| [ASK-025](../planning/issues/ASK-025.md) | Prepare CLI binary distribution and install documentation | M4 | P1 | ASK-014, ASK-015, ASK-023, ASK-024 |
| [ASK-026](../planning/issues/ASK-026.md) | Complete the OSS visibility and first-release approval gate | M4 | P0 | ASK-002, ASK-012, ASK-013, ASK-014, ASK-015, ASK-021, ASK-022, ASK-023, ASK-024, ASK-025 |
| [ASK-027](../planning/issues/ASK-027.md) | Design opt-in symbolic-link installation | M5 | P2 | ASK-012, ASK-013, ASK-022 |
| [ASK-028](../planning/issues/ASK-028.md) | Design a separate remote skill acquisition layer | M5 | P2 | ASK-008, ASK-012, ASK-013, ASK-023 |
| [ASK-029](../planning/issues/ASK-029.md) | Design managed upgrades and safe uninstall | M5 | P2 | ASK-011, ASK-012, ASK-013 |

## Autonomous implementation workflow

The maintainer may authorize an agent to work through a complete milestone rather than stopping after each issue. Each dependency-ready issue must still have focused changes, required checks, self-review, and an individually understandable PR. Squash merge is the default, and official `gh stack` is preferred only for linearly dependent PRs. See [Autonomous development policy](autonomous-development.md) for merge eligibility, QA comments, GitHub settings, and the prohibition on publication without explicit approval. M5 remains out of scope for pre-publication milestone runs.

## Initial execution order

Start ASK-001. ASK-002 and ASK-008 can be researched in parallel. Then establish CI/schema/resolution, complete the core target evidence and validation, and proceed to planner -> executor -> repeatability/tests -> public API -> CLI. Catalog research can proceed once the schema/resolver are ready.

Do not make everything P0 or everything Ready. M5 stays in Backlog. A dependency is satisfied only after the reviewed work is merged and its issue is resolved. The live issue/PR state wins over this initial plan.

## Acceptance of the first release candidate

A separate Rust consumer installs a benign local fixture into temporary documented core-target roots using the public API. The CLI produces an equivalent plan. Repeated installs are no-ops, conflicts do not clobber content, no skill code runs, and cross-platform failures are reported accurately. Every advertised host/scope/OS combination has evidence and the promised tests.

## Deferred decisions and explicit approvals

Final crate/executable names, licensing, exact Rust pin, measured MSRV, filesystem backend, and release tooling must be verified and reviewed. Do not carry speculative version numbers or dependency claims into code. Public repository visibility, Project visibility, crates.io publication, and binary releases require separate maintainer approval.
