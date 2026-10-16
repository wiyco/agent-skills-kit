# Unpublished library packaging rehearsal

Refs #24 (ASK-024). **WIP — not ready for use.** Rehearsal is not publication approval. The workspace and normalized packaged manifest retain `publish = false`. No cargo publish, login, token exchange, publisher configuration, release tag or artifact upload is part of this procedure.

## Candidate contents and contract

The library manifest declares its repository, existing GitHub API documentation, WIP package README, keywords/category and an explicit include list. No nonexistent docs.rs release or install command is advertised. Runtime core catalog Rust and embedded expanded JSON are included, as are the library source/tests/examples, both original project license texts and the two required saphyr-parser 0.2.2 MIT notices. License/notice copies must match the reviewed root originals byte for byte; future changes cannot silently leave the package stale.

Original material remains MIT OR Apache-2.0 under [ADR 0005](../adr/0005-naming-licensing-and-reuse.md). The [dependency policy](../dependency-policy.md) was adopted in protected PR #107 under the authorized routine workflow, with no exceptions. Neither that self-review nor this rehearsal replaces the maintainer's final rights/security/publication decision. Dependency notices retain their original terms; a future binary notice bundle is separate from this source crate.

The library declares Edition 2024 / MSRV 1.85.0; development tooling is pinned to 1.99.1. There are no optional public features. The no-default-features check therefore uses the same dependency graph; it does not prove an additional backend/feature combination. Target-specific cfg controls native backends. Platform/host/filesystem evidence remains in the test and support matrices, not inferred from the package compiling.

[CHANGELOG.md](../../CHANGELOG.md) records unreleased behavior, not a release. Review public API, CLI/receipt contracts and catalog semantics before choosing an approved version/date. Later pre-1.0 breaking changes require an explicit new minor version and migration notes; compatible fixes use patch versions. No schema version may be silently reused. This issue changes package metadata/assets and verification only, not the library API or declared MSRV.

## Reproduce without publication

Commit the candidate first. Keep only trusted repository code in the commit, and use already installed toolchains and task-owned Cargo state with the locked dependencies cached. The helper rejects tracked changes and unsupported/uninstalled toolchain selections; it never installs Rust or changes proto/global configuration. Untracked local files are not copied.

```sh
CARGO_HOME=/private/tmp/ask-cargo-home RUSTDOCFLAGS='-D warnings -D missing-docs' python3 -B .github/scripts/rehearse-library.py --toolchain 1.99.1
python3 -B -m unittest discover -s .github/scripts/tests -v
```

To inspect the current package list without compiling or uploading, `cargo package -p agent-skills-kit --list --locked` is read-only with respect to repository source; an intentionally uncommitted review needs `--allow-dirty`, but that is not accepted as a clean candidate rehearsal. No publish dry-run command is needed: the official [Cargo package procedure](https://doc.rust-lang.org/cargo/commands/cargo-package.html), reviewed 2026-10-16, provides local packaging/compilation verification without the upload step.

The helper clones the exact clean candidate into an owned temporary directory, with no hardlinks and Git global/system configuration, hooks and templates disabled. It selects only the committed development pin, declared MSRV or stable CI toolchain. All Cargo operations are offline/locked and use a fresh temporary target directory. It then:

1. Checks legal-copy identity and required files against cargo package --list.
2. Runs cargo package with its normal verification enabled, retaining publish=false.
3. Inspects every actual archive entry before extraction: bounded count/unpacked size, regular files only, canonical contained paths, no links/special entries, duplicate paths, traversal, streams or unexpected hidden files. The only hidden file allowed is generated `.cargo_vcs_info.json`.
4. Requires exact equality between listed/archived paths, a clean matching VCS revision, and byte-identical packaged runtime assets, README and notices.
5. Checks the unpacked library with all targets/no-default-features, runs its all-feature tests and strict rustdoc, then runs the existing independent consumer against the **unpacked artifact**, not the repository library or CLI. Its existing lockfile and source are retained; only its path dependency is redirected.
6. Prints candidate SHA, actual rustc/host information, relative file inventory, artifact size/SHA-256 and success status, then removes the owned snapshot/build/archive/consumer. Digests are verification records, not public downloads. No user skill stores, credentials, skill scripts or paid model calls are used.

This is an inspector for Cargo's own candidate archive, not a remote/archive acquisition feature. Required unit fixtures cover links/special files, traversal/absolute and Windows stream/backslash paths, hidden credential paths, canonical aliases, duplicate entries, file/size budgets and unreviewed-toolchain refusal without launching a subprocess. A clean package is not an exhaustive malware/privacy or downstream-host review.

## Actual evidence and native CI

Initial local checkpoint **189e2017c031911b2fdf74d4f17d62d1d0dd34d5**, 2026-10-16 UTC: macOS 15.8.1 / aarch64-apple-darwin, Rust/Cargo 1.99.1, Python 3.15.0. The rehearsal **passed**: 73 files, 194701 bytes, SHA-256 `004efa5aaf1247e9f853a83bc1a5af8327760b753da76e13f3cb703f0fd503f9`, package verification and unpacked-library tests/docs plus all 3 independent-consumer tests (186 tests including the consumer). This measures that checkpoint, not later revisions; final PR QA records the final source/digest and native CI separately. The archive was not uploaded or retained as a public release.

Existing required native Linux/macOS/Windows jobs now run the development-toolchain rehearsal. Linux MSRV runs it in the compatibility job; macOS/Windows run it on the declared native MSRV as well. Windows already installs its MSRV for backend checks; only macOS needs an additional CI-only installation for this measurement. Required context names/protections are unchanged. CI PR checkouts may be GitHub merge revisions; each report records its actual source SHA separately from the PR head status. Do not claim a runner/toolchain outcome until its final-revision check succeeds. No local MSRV installation or execution is implied by the local 1.99.1 result.

## Registry and future automation review

Official [Cargo publishing](https://doc.rust-lang.org/cargo/reference/publishing.html), [manifest](https://doc.rust-lang.org/cargo/reference/manifest.html) and [rust-version](https://doc.rust-lang.org/cargo/reference/rust-version.html) documentation was reviewed on 2026-10-16. Publication is permanent/versioned and distinct from packaging. Keep publish=false until a separately approved release change; approve the exact source/version, notices, dependency policy, CI and package digest before any upload. The CLI must use the same reviewed library, in library-first publication order after the library is actually available.

Read-only registry checks on 2026-10-16, with a project-identifying User-Agent, returned HTTP 404 and structured crate-does-not-exist responses for [agent-skills-kit](https://crates.io/api/v1/crates/agent-skills-kit) and [agent-skills-kit-cli](https://crates.io/api/v1/crates/agent-skills-kit-cli). Initial generic requests returned 403 and were not treated as availability evidence. Names are **not reserved**; repeat both checks immediately before a real approval/publication decision. No account/token was used.

Current [crates.io Trusted Publishing docs](https://crates.io/docs/trusted-publishing) were reviewed through their [host-owned page source at 26c169c](https://github.com/rust-lang/crates.io/blob/26c169c48c6a1120a2798f0429523ad377d4164c/svelte/src/routes/docs/trusted-publishing/+page.svelte), because the registry page was JavaScript-only in the reader. Prerequisites still require an already-published crate and owner; the initial publish needs an API token. Current docs include GitHub and beta GitLab support; the older July 2025 announcement's GitHub-only statement is not used as current scope. Later GitHub setup binds repository/workflow/optional environment and uses short-lived OIDC-derived credentials. None is configured or requested here.

[Release-plz Quickstart](https://release-plz.dev/docs/github/quickstart), reviewed 2026-10-16, distinguishes release PR creation from actual publishing/tagging and documents built-in trusted-publishing support plus the same initial-crate limitation. Its example publishing/write-permission workflow is not installed or executed. For this pre-publication phase, ordinary reviewed Conventional PRs, the unreleased changelog and the pinned Cargo rehearsal are sufficient. Any future automation selection/version/action SHA, environment approval, token/OIDC grant, first-publish procedure and release trigger require separate review and authorization. No unverified installation path or unused release dependency is introduced.
