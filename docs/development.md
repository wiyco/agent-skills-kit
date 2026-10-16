# Local development

## Supported workflow

Use rustup + Cargo for Rust while continuing to use proto for the rest of the developer toolchain. Docker is not required for local development or normal tests. Native OS testing is preferred for this filesystem-focused project.

The official proto Rust plugin delegates to rustup, references rustup toolchains, and does not create Cargo/rustup shims. It recommends `rust-toolchain.toml` for runtime toolchain selection. Ensure the Cargo bin directory is on PATH. [Official proto Rust plugin](https://github.com/moonrepo/rust-plugin).

## Single source of truth

The exact development pin is Rust 1.99.1 in `rust-toolchain.toml`, with rustfmt and Clippy. The installed components were already available at bootstrap; no installation or global default change was needed. `rust-toolchain.toml` is checked in and used by both proto users and plain-rustup contributors. [Rustup overrides](https://rust-lang.github.io/rustup/overrides.html).

Rust does not need a duplicate pin in `.prototools`. Existing Node.js/pnpm/Terraform entries may remain in local or shared proto configuration. If maintainers later require a Rust entry in `.prototools`, add an automated consistency check or generation step; do not maintain two unrelated pins manually.

The development toolchain pin is not the MSRV. The bootstrap workspace's measured MSRV is 1.85.0, declared separately in Cargo package metadata after its compatibility job passed. See [the MSRV policy](adr/0004-msrv-policy.md) and [Cargo rust-version](https://doc.rust-lang.org/cargo/reference/rust-version.html).

## Prerequisites

Rustup, a suitable native compiler/linker toolchain for the OS, Git, and an editor with Rust support. Windows contributors need the toolchain/linker components appropriate to the chosen Rust target; macOS contributors need the relevant command-line developer tools. Record tested setup details in ASK-001 rather than assuming Rust alone installs every native prerequisite.

GitHub CLI is useful for issue/PR work. Node.js is only required to run the separate provisioning bundle, not to build or use the Rust product. No package manager dependency installation is required by that bundle.

## Fresh checkout

Ensure the Cargo bin directory (normally `$HOME/.cargo/bin`, or the bin directory of a custom `CARGO_HOME`) is on PATH. Proto delegates Rust management to rustup; no proto command is required to build. Plain rustup users use the same commands. With rustup installed, the repository toolchain file selects the exact version and required components when Cargo runs. An offline checkout requires that toolchain and components to be installed already.

From the checkout:

```sh
rustup show active-toolchain
rustc --version
cargo --version
cargo check --workspace --locked
cargo test --workspace --locked
```

Run commands from the checkout so rustup uses its committed toolchain file. Do not change the global default toolchain unnecessarily.

## Daily checks

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo doc --workspace --no-deps --locked
```

The packages are `agent-skills-kit` (library) and `agent-skills-kit-cli` (CLI, executable `agent-skills-kit`). Run `cargo run -p agent-skills-kit-cli --locked` to print the library bootstrap status. Command parsing and installation arrive in later issues; names and licensing are recorded in ADR 0005. No external Rust dependencies or feature flags are present. Both packages inherit the measured MSRV from the workspace; the development toolchain remains independently pinned.

The original `crates/agent-skills-kit-cli/tests/fixtures/bootstrap.stdout` fixture records the bootstrap output. The integration test launches only the compiled CLI and compares its output; it performs no installation, executes no skill content, and makes no writes to agent stores.

See [ASK-001 verification](verification/ask-001.md) for the tested setup and limitations.

## CI policy

The `Rust` workflow runs native Linux/macOS/Windows check/test jobs on the development pin; Linux formatting, Clippy, and rustdoc checks; a measured MSRV job; and a latest-stable compatibility check. The quality job also proves formatting and lint failures are rejected in temporary copies. There are currently no feature flags; add no-default-features checks when features are introduced.

Checkout and cache actions are pinned to verified release commit SHAs. Dependabot proposes weekly action updates for review. Target cache keys include OS, architecture, the exact development toolchain, Cargo manifests, Cargo.lock, and job purpose, without broad restore fallbacks. Compatibility jobs run without caches. PR permissions are read-only, checkout does not persist credentials, jobs have bounded timeouts, and superseded runs are cancelled. No secrets or `pull_request_target` execution is used. [GitHub Actions security guidance](https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions).

Docker/devcontainers and task runners may be added later for a demonstrated need. They do not replace native Windows/macOS filesystem tests. Avoid introducing Node/pnpm/Turbo solely to orchestrate Cargo.
