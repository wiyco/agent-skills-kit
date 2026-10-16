# Agent Skills Kit (Rust)

> **WIP — not ready for use.** This is a source-only development preview, not a published or supported Rust library or CLI tool. Do not use it to install skills into real user or agent directories. No crates, installers or binary releases are available. Public contribution intake, reporting arrangements and release readiness remain unfinished.

A library-first Rust toolkit for installing local Agent Skills into the documented directories of selected AI coding tools.

**Status: unreleased development.** The library provides catalog/context resolution, bounded inert validation, read-only planning, native no-clobber copying and versioned observational receipts. Native regression evidence covers Linux, macOS/APFS and Windows Server 2025 x64/NTFS; see [execution and limits](docs/execution.md) and [ASK-012 evidence](docs/verification/ask-012.md). Other Windows configurations and actual host discovery remain unvalidated. The CLI offers catalog inspection, validation, explicit-context planning and approved native installation with optional terminal selection; see [CLI usage](docs/cli.md). No crate or binary release is available. Source visibility does not imply a completed release candidate. The library is `agent-skills-kit`; the CLI package is `agent-skills-kit-cli`, with executable `agent-skills-kit`.

## What it does

Select a local skill, one or more target tools, and a scope. The reusable Rust library resolves documented destinations, validates the source, creates a read-only plan, and performs an explicitly approved copy. A thin CLI uses the same public API.

The core targets are **Codex, Claude Code, and OpenCode**. The [bundled target matrix](docs/targets.md) lists additional documented adapters and their resolver platforms. [Open issues](https://github.com/wiyco/agent-skills-kit/issues) track unfinished catalog and release-readiness work. A research candidate is **not** a supported target.

Selecting a tool determines where files are written. It does **not** ensure that only that tool can discover them: some tools read shared or compatibility directories. See [architecture](docs/architecture.md).

## Start here

- [Product plan and milestones](docs/plan.md)
- [Architecture and safety boundaries](docs/architecture.md)
- [Catalog schema and evidence](docs/catalog.md)
- [Explicit-context path resolution](docs/resolution.md)
- [Local development](docs/development.md)
- [AI agent instructions](AGENTS.md)
- [Target candidates and evidence](docs/research/target-candidates.md)
- [GitHub Project workflow](docs/project-workflow.md)
- [Release gate](docs/release.md)

The task catalog is in [planning/manifest.json](planning/manifest.json); individual task specifications are in [planning/issues](planning/issues). GitHub Issues become the execution source of truth after import. Stable `ASK-###` keys are not GitHub issue numbers.

## Workspace layout

```text
crates/
  agent-skills-kit/  # reusable library
  agent-skills-kit-cli/          # thin CLI (agent-skills-kit executable)
```

For maintainers reviewing the WIP source, build and inspect it with the pinned toolchain. These are development checks, not installation instructions or an invitation to use the library/CLI:

```sh
cargo check --workspace --locked
cargo test --workspace --locked
cargo run -p agent-skills-kit-cli --locked -- --help
cargo run -p agent-skills-kit-cli --locked -- targets list
```

These CLI examples do not install skills. See [local development](docs/development.md) for setup and verification, [contributing](CONTRIBUTING.md) for changes, and [security reporting](SECURITY.md) before disclosing vulnerabilities. The maintainer Project is private; the issue list is the accessible roadmap when the repository is public.

## Development direction

Rustup and Cargo provide the Rust toolchain and build workflow. Existing proto-based developer environments remain supported. Docker, Node.js, pnpm, moon, and Turbo are not required by the product build. The separate repository-provisioning bundle uses dependency-free Node.js solely to drive GitHub CLI.

## Scope of v0.1

Local directory input, project/user scope where documented, copy mode, explicit plans, no-clobber defaults, typed library errors, deterministic reports, cross-platform tests, and a CLI. Symbolic links, remote fetching, destructive replacement, updates, and uninstall are post-MVP work.

## License

Original project material is licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at your option. Third-party material retains its own terms; see [attribution](docs/attribution.md). License adoption does not authorize publication. See [ADR 0005](docs/adr/0005-naming-licensing-and-reuse.md) for naming and licensing decisions.

This project is independent of the agent products it targets and makes no affiliation claim.

See the [public API guide](docs/api.md) and [standalone consumer](examples/external-consumer) for library use without the CLI.

The [generated target matrix](docs/targets.md) separates documented rules/resolver evidence from unverified real-host discovery. Product-specific source and variant limits are in the [catalog review](docs/catalog.md). The [ASK-020 outcomes](docs/research/ask-020-outcomes.md) reconcile workspace-based and renamed products with explicit deferred capabilities.
