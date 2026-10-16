# Agent Skills Kit

A **planned, library-first Rust toolkit** for installing local Agent Skills into the documented directories of selected AI coding tools.

**Status:** Cargo workspace bootstrap. The library and CLI skeleton compile, but installation is not implemented and there is no released crate or CLI. The repository is private during development and is intended for a future OSS release. Project, crate, and executable names remain provisional until the naming review.

## What it will do

Select a local skill, one or more target tools, and a scope. The reusable Rust library resolves documented destinations, validates the source, creates a read-only plan, and performs an explicitly approved copy. A thin CLI uses the same public API.

The initial targets are **Codex, Claude Code, and OpenCode**. The backlog includes broader editor, CLI, and application-workspace research. A research candidate is **not** a supported target.

Selecting a tool determines where files are written. It does **not** ensure that only that tool can discover them: some tools read shared or compatibility directories. See [architecture](docs/architecture.md).

## Start here

- [Product plan and milestones](docs/plan.md)
- [Architecture and safety boundaries](docs/architecture.md)
- [Local development](docs/development.md)
- [AI agent instructions](AGENTS.md)
- [Target candidates and evidence](docs/research/target-candidates.md)
- [GitHub Project workflow](docs/project-workflow.md)
- [Release gate](docs/release.md)

The task catalog is in [planning/manifest.json](planning/manifest.json); individual task specifications are in [planning/issues](planning/issues). GitHub Issues become the execution source of truth after import. Stable `ASK-###` keys are not GitHub issue numbers.

## Workspace layout

```text
crates/
  agent-skills-kit/  # reusable library; provisional crate name
  skillctl/          # thin CLI; provisional executable name
```

Run `cargo check --workspace --locked` and `cargo test --workspace --locked` from the checkout. `cargo run -p skillctl --locked` prints the library bootstrap status. See [local development](docs/development.md) for setup and verification.

## Development direction

Rustup and Cargo provide the Rust toolchain and build workflow. Existing proto-based developer environments remain supported. Docker, Node.js, pnpm, moon, and Turbo are not required by the product build. The separate repository-provisioning bundle uses dependency-free Node.js solely to drive GitHub CLI.

## Scope of v0.1

Local directory input, project/user scope where documented, copy mode, explicit plans, no-clobber defaults, typed library errors, deterministic reports, cross-platform tests, and a CLI. Symbolic links, remote fetching, destructive replacement, updates, and uninstall are post-MVP work.

## License

The proposed license is `MIT OR Apache-2.0`, subject to maintainer approval in ASK-002. No license adoption or public release is claimed by this planning repository.

This project is independent of the agent products it targets and makes no affiliation claim.
