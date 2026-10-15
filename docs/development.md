# Local development

## Supported workflow

Use rustup + Cargo for Rust while continuing to use proto for the rest of the developer toolchain. Docker is not required for local development or normal tests. Native OS testing is preferred for this filesystem-focused project.

The official proto Rust plugin delegates to rustup, references rustup toolchains, and does not create Cargo/rustup shims. It recommends `rust-toolchain.toml` for runtime toolchain selection. Ensure the Cargo bin directory is on PATH. [Official proto Rust plugin](https://github.com/moonrepo/rust-plugin).

## Single source of truth

ASK-001 creates an exact version pin in `rust-toolchain.toml` with rustfmt and Clippy. Do not copy an unverified numeric Rust version from earlier discussion. `rust-toolchain.toml` is checked in and used by both proto users and plain-rustup contributors. [Rustup overrides](https://rust-lang.github.io/rustup/overrides.html).

Rust does not need a duplicate pin in `.prototools`. Existing Node.js/pnpm/Terraform entries may remain in local or shared proto configuration. If maintainers later require a Rust entry in `.prototools`, add an automated consistency check or generation step; do not maintain two unrelated pins manually.

The development toolchain pin is not the MSRV. Determine and test the library's minimum supported Rust version separately before declaring it in package metadata. [Cargo rust-version](https://doc.rust-lang.org/cargo/reference/rust-version.html).

## Prerequisites

Rustup, a suitable native compiler/linker toolchain for the OS, Git, and an editor with Rust support. Windows contributors need the toolchain/linker components appropriate to the chosen Rust target; macOS contributors need the relevant command-line developer tools. Record tested setup details in ASK-001 rather than assuming Rust alone installs every native prerequisite.

GitHub CLI is useful for issue/PR work. Node.js is only required to run the separate provisioning bundle, not to build or use the Rust product. No package manager dependency installation is required by that bundle.

## Fresh checkout

Before ASK-001 is merged, this is a planning-only repository. After the workspace exists:

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

The final workspace package/executable names are selected in ASK-002. Add precise `cargo run -p ...` examples once those packages exist, rather than publishing non-working examples.

## CI policy

Native Linux/macOS/Windows tests; formatting, Clippy, and rustdoc checks; a measured MSRV job; and a latest-stable compatibility check. Pin third-party actions to verified SHAs and update them through reviewed PRs. Keep pull-request permissions read-only and avoid secrets in untrusted jobs. [GitHub Actions security guidance](https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions).

Docker/devcontainers and task runners may be added later for a demonstrated need. They do not replace native Windows/macOS filesystem tests. Avoid introducing Node/pnpm/Turbo solely to orchestrate Cargo.
