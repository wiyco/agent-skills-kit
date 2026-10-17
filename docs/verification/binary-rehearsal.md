# Unpublished native CLI binary rehearsal

This is WIP build verification, not an available release or installation recommendation. No public download, package-manager installer, release badge, tag, upload, signing credential or publisher is created. Workspace packages retain `publish = false`.

## Recipe and scope

The reviewed equivalent to cargo-dist is the existing Rust/Cargo development pin in `rust-toolchain.toml` (1.99.1), locked dependencies, Python 3.11+ standard-library archive/checksum handling and schema 1 in `.github/binary-rehearsal.json`. No additional distribution tool is installed. `rustup which` resolves already-installed pinned executables; the local rehearsal never installs a toolchain. CI installs the declared pin on ephemeral native runners.

The [cargo-dist introduction](https://axodotdev.github.io/cargo-dist/book/introduction.html#plan-build-host-publish-announce) and [configuration reference](https://axodotdev.github.io/cargo-dist/book/reference/config.html), reviewed 2026-10-17, distinguish building from hosting/publishing/announcing. cargo-dist can build without distribution, but generated release workflows can create/upload releases and installers. The smaller equivalent recipe is sufficient for this pre-publication checkpoint. Reconsider cargo-dist and pin its reviewed version in a separately approved distribution change; do not activate its generated hosting pipeline here.

[Cargo build](https://doc.rust-lang.org/cargo/commands/cargo-build.html), sections Compilation Options, Manifest Options and Feature Selection (reviewed 2026-10-17), defines the native target, release profile and locked/offline inputs used here. This is a repeatable recipe, not a promise of bit-identical output across toolchains, SDKs, runner images or machines. Archive metadata is normalized and source/cache paths are remapped; build environment details and candidate SHA remain in the inventory.

| Native compiler host | Archive | Evidence boundary |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | tar.gz | Native Ubuntu CI; measured runner/glibc only |
| `aarch64-apple-darwin` | tar.gz | Native Apple Silicon macOS; measured runner/local OS only |
| `x86_64-pc-windows-msvc` | zip | Native Windows x64 MSVC CI; measured runner only |

These are rehearsal targets, not supported public distribution promises. Other CPUs, musl, Windows ARM64, macOS Intel, older deployment targets and different Linux runtime baselines are unverified. No cross-compilation result substitutes for execution. `platform.libc_ver()` is recorded as an observation, not an ELF symbol audit or minimum-runtime guarantee. CI evidence must be green before a target is described as measured.

## Run and inspect locally

From a clean committed checkout with the pin installed and locked registry sources already cached:

```sh
python3 -B .github/scripts/rehearse-binary.py
```

The helper creates its own temporary clone of the exact Git commit, a fresh build directory and a private bundle. It builds the existing CLI using its direct same-snapshot library dependency and runs optimized workspace tests. It does not introduce another installer or registry. The release-profile recipe explicitly retains debug information (`strip=none`); stripping is not a verification claim. An initial macOS checkpoint completed tests and smoke checks but emitted `rust-objcopy` missing-LLVM warnings; the explicit unstripped recipe removes reliance on that optional utility without changing the user's toolchain.

Artifacts are named `agent-skills-kit-{version}-{native-target}-{commit-prefix}.{tar.gz|zip}`. Bundles contain the executable, project MIT/Apache texts, README, a component inventory and byte-preserved locked-dependency notices. The inventory includes declared licenses, source repositories, crate checksums and notice hashes. It conservatively includes reachable non-development Cargo graph components, including build dependencies, rather than asserting every listed component is linked at runtime. A dependency's `.licenses` directory is mapped to `notice-files` with the original/mapped names recorded. This inventory is not a certified SBOM, license opinion or provenance attestation.

A private `SHA256SUMS` is read back before extraction. All member names/types/counts/sizes are validated before writing; traversal, aliases, links, special files and duplicate paths are refused. Contents are compared byte-for-byte by SHA-256. The unpacked binary is copied to a newly created test-owned `installation/bin`, assigned executable permissions there and invoked by absolute path with `--help` and `--version`. Its version and hash must match. Nothing is added to PATH or a real home/agent directory. All candidate artifacts and installation state are deleted after the JSON evidence is printed.

These are internal isolated placement instructions only. Public download/install and `cargo install` instructions remain absent until their corresponding release actually exists. There is no installer, updater, elevation request or global environment mutation.

## CI, negative cases and publication gates

The existing three native jobs run the same helper under read-only workflow permissions. No artifact upload, release creation, OIDC or write permission is enabled. Existing library/MSRV, external-consumer, formatting, Clippy, documentation and dependency gates remain required. Binary release-mode execution uses the development pin; it is separate from the library MSRV package evidence.

`python3 -B -m unittest discover -s .github/scripts/tests -v` includes original inert archive fixtures: tampered checksums rejected before extraction, traversal/hidden/backslash entries, symlinks and duplicate ZIP members refused, valid regular content preserved, and unreviewed CPU/ABI/configuration rejected. Existing shared archive tests retain entry/count/size and type bounds. These tests execute no downloaded skill or archive payload.

The PR's separate QA Evidence comment records the exact final SHA, actual native CI runs, local environment, checksums and limitations. An earlier checkpoint is not evidence for a later candidate. Native smoke execution does not test downloaded-file quarantine, Gatekeeper, SmartScreen, signing/notarization, power-loss durability, all filesystems, oldest OS runtimes or installation on an end-user machine without development tools. No universal static-linking claim is made.

Before distribution, the maintainer must approve the exact version/commit, runtime baselines, notice/license review, signing and provenance decisions, the publication destination and its protected approval workflow. Obtain clean final-release builds rather than promoting PR caches. [Release gates](../release.md) and ASK-026 remain authoritative; this rehearsal authorizes none of those actions.
