# ADR 0004: Measured MSRV independent of the development pin

Status: accepted; measurements remain revision/platform-specific.

The workspace uses Rust 1.85.0 as its measured minimum supported Rust version. Both packages inherit `rust-version` from workspace metadata. The repository development pin remains Rust 1.99.1 in `rust-toolchain.toml`; it selects current formatting and lint behavior, not the minimum compiler.

Rust 1.85.0 is the Edition 2024 baseline according to the [official release announcement](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/) (reviewed 2026-10-16). Before declaring support, the [candidate compatibility job](https://github.com/wiyco/agent-skills-kit/actions/runs/38873417914) passed workspace all-target/all-feature check and tests at revision `89fa4f072e3ed57607760bf03e1070aabafb0c09`. This measures the bootstrap code and its path-only dependency graph, not future code or dependencies.

CI separately tests the MSRV and latest stable with an explicit job-local rustup override, leaving the development file unchanged. The MSRV job asserts that its tested version equals Cargo metadata. Native platform tests use the development pin; Windows backend MSRV check/test/consumer coverage was subsequently added under ASK-012. The [ASK-024 package rehearsal](../verification/library-package.md) configures unpacked-library/consumer verification on the declared MSRV for Linux/macOS/Windows, with actual final-revision results recorded in PR QA. Historical bootstrap evidence alone does not establish current native compatibility or future platform-specific APIs; no Windows MSRV Clippy result is implied.

A dependency or language/API change requiring a newer compiler must update the declared MSRV, compatibility matrix, documentation, and evidence in a reviewed PR. Check the dependency's actual compiler requirements before adoption. Do not lower the declaration without passing tests, or silently treat the dev pin as an MSRV claim. The local developer toolchain and proto configuration need no global changes to run CI.
