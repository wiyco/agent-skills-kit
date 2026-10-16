# Dependency and CI supply-chain policy

Development evidence for ASK-023, reviewed 2026-10-16. This does not authorize publishing, establish an absence of vulnerabilities or replace the separate [release gate](release.md). Production library/CLI behavior remains local and synchronous; the network-enabled checker is maintenance tooling only.

## Enforced policy

`deny.toml` checks the locked whole workspace, all features and all target-specific dependencies, including development dependencies and unpublished workspace members. Do not use `--exclude-unpublished` while `publish = false`. Allow only MIT, Apache-2.0 and Unicode-3.0 license branches, with 0.95 detection confidence. An OR expression needs an allowed branch; an AND expression needs every required term. This is not relicensing: retain upstream notices, including the two [saphyr parser notices](attribution.md), and the [Unicode V3 copyright/permission notice](https://www.unicode.org/license.txt) where required. Unicode-3.0 is required by the existing unicode-ident dependency; it is not a broad permission to copy third-party material.

Reject known vulnerability/notice findings, unmaintained advisories, yanked versions, duplicate dependency versions, wildcard requirements, unknown registries and Git dependencies. The only external registry allowed is crates.io. Workspace path dependencies remain local; the CLI now declares the library version as well as its path, rather than an unconstrained wildcard. Cargo.lock and the standalone consumer's Cargo.lock stay committed; all checks use locked resolution. No async runtime, network acquisition or terminal library was added to the production dependency graph.

**Current exceptions: none.** Do not suppress a finding merely to turn CI green. Any proposed advisory ignore, license exception/clarification, source allowance or duplicate-version waiver requires a focused reviewed PR with the affected package/version or advisory ID, owner, concrete reason, evidence, review date and expiry/removal condition. Record that decision here and in the tool configuration. An exception without these fields is not approved. Re-run the actual failing check and the relevant tests; publication-critical safety issues remain blockers.

## Tool and update trust

Selected tool: **cargo-deny 0.20.2**, official release **2026-07-09**. [Release notes](https://github.com/EmbarkStudios/cargo-deny/releases/tag/0.20.2) and [tagged manifest](https://github.com/EmbarkStudios/cargo-deny/blob/0.20.2/Cargo.toml) show active maintenance, MIT OR Apache-2.0 licensing and tool MSRV 1.88.0; this tool's requirement does not raise the library MSRV 1.85.0. The existing development Rust 1.99.1 satisfies it. It covers advisories, licenses, bans and sources in one tool, avoiding a redundant production dependency or second audit engine. See the [official CLI](https://embarkstudios.github.io/cargo-deny/cli/index.html), [advisories configuration](https://embarkstudios.github.io/cargo-deny/checks/advisories/cfg.html) and [licenses configuration](https://embarkstudios.github.io/cargo-deny/checks/licenses/cfg.html). Tagged binary help and actual tests, rather than obsolete example flags, determine this pinned invocation.

Official GitHub release API digests were reviewed before downloading; archives must match these committed expectations before execution. Matching a digest protects the reviewed artifact identity, not against a compromised upstream maintainer.

| Reviewed release archive | SHA-256 | Execution boundary |
| --- | --- | --- |
| `cargo-deny-0.20.2-aarch64-apple-darwin.tar.gz` | `fe67d82a10d8597a3549364cb733a3f9cc1bfff9031b7ae46384a9f2a72090c3` | Actual local Apple Silicon test; temporary binary only |
| `cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz` | `9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f` | Linux quality CI; actual final-SHA results belong to PR QA |

The existing required `quality` context downloads the pinned official Linux archive over HTTPS, checks SHA-256, extracts only its executable into RUNNER_TEMP and runs `.github/scripts/verify-dependencies.py`. The check uses a separate task-owned CARGO_HOME, not a cached user credential directory. No shell installer, global PATH/proto mutation, privileged token or tool cache is introduced. Local macOS verification used the official archive at the digest above; no Windows tool execution is claimed. Updates must review the upstream release, license/MSRV, digest and negative fixtures; a version string alone is insufficient.

Dependabot opens weekly version PRs for GitHub Actions and for both Cargo lockfile roots, with Conventional titles. Existing update configuration is extended, not duplicated. Bot PRs must pass the same checks/review/QA gates; no bot auto-merge or broad write workflow is added. [Dependabot options](https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference), reviewed 2026-10-16, describe these ecosystems/directories and commit-message options. Version updates are distinct from the repository's separately disabled automatic Dependabot security-update setting.

## Positive and controlled negative verification

Use an existing reviewed tool binary in an owned temporary directory and explicit task-owned Cargo state:

```sh
CARGO_HOME=/private/tmp/ask-cargo-home python3 -B .github/scripts/verify-dependencies.py --cargo-deny /private/tmp/ask23-tools/cargo-deny
python3 -B -m unittest discover -s .github/scripts/tests -v
cargo tree --workspace --locked --edges normal,build
cargo package -p agent-skills-kit --list --allow-dirty --locked
```

The real check must succeed first, fetching the official RustSec database and checking the whole locked graph. It then proves (1) an empty allowed-license list rejects licenses with the measured exit 4 and `error[rejected]`, and (2) an original **fictional** advisory rejects an existing package with exit 1 and the synthetic ID. The second test uses a separate no-hardlink local clone of the just-fetched database, with Git global/system configuration, hooks and templates disabled for fixture operations. It commits only the original fixture into that disposable clone and runs the negative check offline. The production database and repository are not edited. Both lockfile hashes must remain unchanged. Success, a panic, malformed configuration or an unrelated error cannot satisfy a negative fixture; offline classification tests cover these false-positive cases. The synthetic ID is not a real RustSec report and is never published upstream.

Actual local baseline on 2026-10-16: advisories/bans/licenses/sources passed against RustSec database revision `f28da8c5d386e507fac79153655184056a20c054`; both controlled rejections passed. The first exploratory unquoted advisory date was rejected by the tool with a parser panic; the committed fixture uses its required quoted date. That panic was not counted as a passing negative test. Online checks can fail on service availability; do not convert such failures to green. Offline fixture tests are separate from live advisory coverage. Database freshness is limited to seven days when fetching is disabled; CI normally fetches current data.

## Direct dependency necessity

Versions and licenses were inspected from locked Cargo metadata and the dependency tree. Existing per-dependency maintenance/MSRV reviews remain in their implementation PRs and design documents; a clean advisory result alone does not demonstrate active maintenance or correctness.

| Library dependency | Concrete need in the library, independent of the CLI |
| --- | --- |
| serde 1.0.229 / serde_json 1.0.154 | Versioned catalog/context/receipt models and bounded interoperable JSON; also usable by non-CLI Rust consumers |
| saphyr-parser 0.2.2 | Event-level bounded YAML frontmatter parsing, duplicate/alias/shape diagnostics; reviewed alternatives and notices in [validation](validation.md) |
| unicode-normalization 0.1.25 / caseless 0.2.2 | Canonical normalization and full case-fold collision checks for safe portable planning; sparse update history is not evidence of a current advisory, and upgrades require renewed Unicode/boundary tests |
| blake3 1.8.7 | Bounded inventory/copy verification and receipt digests, pure feature configuration |
| rustix 1.1.5 / libc 0.2.190 (Unix only) | Retained descriptors, no-follow operations and native exclusive commit/permissions; platform-specific unsafe boundaries remain tested separately |
| windows-sys 0.61.2 (Windows only) | Native retained-handle NTFS creation, access control and no-replace operations; no Unix string simulation substituted |

The CLI has only the public library and serde_json for rendering; it does not own a second registry/executor. Transitive licenses are checked too: for example unicode-ident's required Unicode-3.0 term is not hidden by its MIT/Apache alternatives. No dependency is added by this issue.

## CI, caches, packages and future artifacts

All PR jobs declare read-only repository contents, with checkout credential persistence disabled. No publishing credential or OIDC write grant is exposed. No pull_request_target job checks out/executes untrusted code. The title workflow reads PR metadata; it does not execute the title as shell code. Upstream tag identities verified 2026-10-16: checkout **v7.0.1** = `3d3c42e5aac5ba805825da76410c181273ba90b1`; cache **v6.1.0** = `55cc8345863c7cc4c66a329aec7e433d2d1c52a9`. Full-SHA pinning does not eliminate the need to review action code or runner-image changes. [GitHub secure use](https://docs.github.com/en/actions/reference/security/secure-use).

The existing cache stores build output under target only, keyed by runner OS/architecture, development pin, manifests/lockfile and native/quality role, without broad restore keys. It is an optimization, never release evidence. Caches must contain no credentials or private host data: forks/PRs can read accessible base caches. GitHub's [cache scope rules](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching) isolate PR merge-ref caches from other/default branches. Future release builds must start clean and must not promote PR caches/artifacts into release assets. No current workflow uploads build artifacts or publishes packages; the manual evidence report writes only a job summary. Keep any later upload/attestation job separate and explicitly approved.

Package-list inspection included the embedded expanded catalog and Rust core catalog and found no local user-store data in the listed paths. It also confirmed **missing package-level README/license/notice inclusion and repository/documentation metadata**, which remain ASK-024 blockers before any distribution. A file listing is not a compiled packaged-consumer test or exhaustive privacy audit. Follow the [Cargo package procedure](https://doc.rust-lang.org/cargo/commands/cargo-package.html); publish=false remains in place.

Before any binary/crate release, record exact source commit, toolchains/targets, lockfile dependencies/checksums/licenses, notice bundle and artifact SHA-256. Produce a reviewed SPDX or CycloneDX SBOM for the actual packaged graph and review provenance/attestation tied to the approved build; Cargo.lock and cargo tree are inputs, not a finished SBOM or provenance claim. OIDC/release signing and upload permissions must not be enabled in untrusted PR jobs. No SBOM, attestation or successful release is fabricated by this policy.

Read-only repository API inspection on 2026-10-16 reported **secret scanning and push protection enabled**, non-provider patterns/validity checks disabled, automatic Dependabot security updates disabled, and **zero repository Actions secrets**. Those observations are not proof that no secret exists or that every organization/environment-level entitlement was examined. No feature/secret/service was activated. Keep human history/privacy review and confidential reporting/publication gates in [public readiness](public-readiness.md) and ASK-026; do not invent unavailable security contacts.
