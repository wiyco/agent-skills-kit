# Local development

## Supported workflow

Use rustup + Cargo for Rust while continuing to use proto for the rest of the developer toolchain. Docker is not required for local development or normal tests. Native OS testing is preferred for this filesystem-focused project.

The official proto Rust plugin delegates to rustup, references rustup toolchains, and does not create Cargo/rustup shims. It recommends `rust-toolchain.toml` for runtime toolchain selection. Ensure the Cargo bin directory is on PATH. [Official proto Rust plugin](https://github.com/moonrepo/rust-plugin).

## Single source of truth

The exact development pin is Rust 1.99.1 in `rust-toolchain.toml`, with rustfmt and Clippy. The installed components were already available at bootstrap; no installation or global default change was needed. `rust-toolchain.toml` is checked in and used by both proto users and plain-rustup contributors. [Rustup overrides](https://rust-lang.github.io/rustup/overrides.html).

Rust does not need a duplicate pin in `.prototools`. Existing Node.js/pnpm/Terraform entries may remain in local or shared proto configuration. If maintainers later require a Rust entry in `.prototools`, add an automated consistency check or generation step; do not maintain two unrelated pins manually.

The development toolchain pin is not the MSRV. Determine and test the library's minimum supported Rust version separately before declaring it in package metadata. [Cargo rust-version](https://doc.rust-lang.org/cargo/reference/rust-version.html).

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

The provisional packages are `agent-skills-kit` (library) and `skillctl` (CLI). Run `cargo run -p skillctl --locked` to print the library bootstrap status. Command parsing and installation arrive in later issues; names and licensing remain subject to ASK-002. No external Rust dependencies or feature flags are present. No `rust-version` is declared because a lower MSRV has not been measured; ASK-003 owns that measurement and CI.

The original `crates/skillctl/tests/fixtures/bootstrap.stdout` fixture records the bootstrap output. The integration test launches only the compiled CLI and compares its output; it performs no installation, executes no skill content, and makes no writes to agent stores.

See [ASK-001 verification](verification/ask-001.md) for the tested setup and limitations.

## CI policy

Native Linux/macOS/Windows tests; formatting, Clippy, and rustdoc checks; a measured MSRV job; and a latest-stable compatibility check. Pin third-party actions to verified SHAs and update them through reviewed PRs. Keep pull-request permissions read-only and avoid secrets in untrusted jobs. [GitHub Actions security guidance](https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions).

Docker/devcontainers and task runners may be added later for a demonstrated need. They do not replace native Windows/macOS filesystem tests. Avoid introducing Node/pnpm/Turbo solely to orchestrate Cargo.
