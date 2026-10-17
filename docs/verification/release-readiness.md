# Pre-publication technical checkpoint

Refs [ASK-026 / #26](https://github.com/wiyco/agent-skills-kit/issues/26). Reviewed 2026-10-17 UTC. **WIP — not ready for use; publication and public contribution intake remain unapproved.** This is an evidence index and decision checklist, not a release announcement, independent approval or authorization to publish. Keep `publish = false`.

## Candidate and milestone reconciliation

Measured implementation baseline: `4d78f77756733ba07bd21a8d26cb1476f6e798a7` (protected squash merge of #109). Its [main CI run 38985720962](https://github.com/wiyco/agent-skills-kit/actions/runs/38985720962) completed successfully. These measurements describe that baseline, not this document's later commit. The audit PR's QA comment and #26 record its final head, checked-out merge revision and, after merge, final main revision/CI. Rebuild and refresh evidence for any separately approved release commit; hashes containing Git provenance cannot be copied to a later revision.

| Milestone | Verified live state / implemented outcome | Evidence |
| --- | --- | --- |
| M0 | Closed; all 4 issues closed / Workflow Done. Workspace/pin, reviewed names/license, native CI/protection and threat model established. | [Milestone 1](https://github.com/wiyco/agent-skills-kit/milestone/1), [ADRs](../adr), [merge policy](../autonomous-development.md) |
| M1 | Closed; all 9 issues closed / Done. Reusable catalog/resolver, inert validation, planning, native no-clobber copy, receipts and independent consumer. | [Milestone 2](https://github.com/wiyco/agent-skills-kit/milestone/2), [API](../api.md), [safety evidence](ask-012.md), [consumer PR #73](https://github.com/wiyco/agent-skills-kit/pull/73) |
| M2 | Closed; both issues closed / Done. Thin scriptable CLI and optional bounded terminal selection. | [Milestone 3](https://github.com/wiyco/agent-skills-kit/milestone/3), [CLI](../cli.md), [#77](https://github.com/wiyco/agent-skills-kit/pull/77), [#78](https://github.com/wiyco/agent-skills-kit/pull/78) |
| M3 | Closed; all 7 issues closed / Done. Documented variant-specific adapters, drift review and opt-in discovery procedures/results. | [Milestone 4](https://github.com/wiyco/agent-skills-kit/milestone/4), [target matrix](../targets.md), [ASK-020 outcomes](../research/ask-020-outcomes.md), [evidence audit](../research/evidence-audit.md), [discovery](host-discovery.md) |
| M4 | ASK-023/024/025 closed / Done; #26 remains open for this audit and explicit human gates. Do not mark the whole milestone complete while those gates remain. | [Milestone 5](https://github.com/wiyco/agent-skills-kit/milestone/5), [#107](https://github.com/wiyco/agent-skills-kit/pull/107), [#108](https://github.com/wiyco/agent-skills-kit/pull/108), [#109](https://github.com/wiyco/agent-skills-kit/pull/109) |
| M5 | All 3 issues remain open / Backlog; not authorized or implemented by this run. | [Milestone 6](https://github.com/wiyco/agent-skills-kit/milestone/6) |

All #26 dependencies (#2, #12–15, #21–25) were checked closed, with their implementation PRs merged. No competing open PR or other assignee was present when the audit began. Project #10's custom Workflow, not built-in Status, was checked against actual issue state.

## Technical release checklist

- [x] Library-first architecture: the CLI and standalone consumer use the public library; no duplicate registry/installation engine. [Architecture](../architecture.md), [API compatibility](../api.md).
- [x] Native Linux/macOS/Windows required CI; strict formatting/Clippy/docs, declared MSRV 1.85.0 and stable compatibility, external-consumer tests. [Testing](../testing.md) states exact native safety scenarios and limits. Windows has real retained-handle NTFS copy/race/ACL/reparse/interruption tests; Unix-looking strings are not substituted for Windows execution.
- [x] Target evidence is current for this checkpoint: 23 canonical product groups, 30 official source URLs, zero offline metadata gaps at 2026-10-17. Recorded source access was 2026-10-16, within the 90-day audit window. All 30 opt-in HTTPS HEAD observations were reachable. This does not reassess document content or prove host discovery; per-rule source sections/applicability and named resolver tests remain authoritative.
- [x] Compared all 50 recorded resolver test references with actual native baseline CI success lines: 50 observed passing on Linux/macOS, 49 on Windows. The one Unix-only Amp copy test is cfg-gated to Linux/macOS; native Windows Amp is explicitly unsupported, not an unrun supported combination. Test references alone were not counted as execution.
- [x] Catalog installation evidence is distinct from real-host execution. Only Codex 0.162.1 Project/User on the measured macOS 15.8.1 setup passed actual discovery. Claude 2.1.292 stopped at onboarding; OpenCode 2.0.25 standalone startup failed under isolation, cause unverified. Other host/platform combinations were not executed. [Actual results and procedures](host-discovery.md); no discovery warning was silently removed.
- [x] Dependency/advisory/license/source policy runs in required quality CI, including controlled negative failures; no exceptions. [Policy and notices](../dependency-policy.md), [attribution](../attribution.md).
- [x] Package contents and independent unpacked-library consumer verified on native Linux/macOS/Windows at development Rust 1.99.1 and MSRV 1.85.0. Each measured library archive had 73 files. At this baseline, development archive: 194683 bytes, SHA-256 `10d90a364f0c9357720ec43fad250eb28a7e83a8e3e410d7646b285279daf1a7`; MSRV archive: 194746 bytes, SHA-256 `c653c365a8d959d4b2cca71f6e54a217dcd0d5b2d39c6b86d62dde59a37b3a66`. Matching measurements across those runners are not a universal reproducibility guarantee. [Package procedure](library-package.md).
- [x] Native optimized CLI archive/checksum/notices and isolated help/version rehearsals passed for the three measured CPU/OS/ABIs. Baseline archive hashes are below. [Recipe and limits](binary-rehearsal.md).
- [x] Naming/license approval is recorded in [ADR 0005](../adr/0005-naming-licensing-and-reuse.md); license/attribution files are packaged. Both crate APIs again returned structured 404 crate-absent responses on 2026-10-17. Names are not reserved or trademark-cleared; recheck immediately before publication.
- [x] No active publishing/upload workflow, public release, public binary, invented download link or release badge. Source-only WIP wording is retained. The library/CLI remain version 0.1.0 unpublished development, not an approved release version.
- [ ] Independent human review of source/history/public-facing material and final rights/security review. Automated author self-review is not this evidence.
- [ ] Verified confidential vulnerability/conduct reporting, alternate handling for maintainer concerns and independent appeals before contribution intake or supported publication. Explicitly deferred under [ADR 0008](../adr/0008-public-source-preview.md); no guessed contacts.
- [ ] Maintainer approval of exact release commit/version, remaining risks, scope/runtime baselines and publication order. Library must be published before a CLI crate that depends on it.
- [ ] Separate crate and binary publication authorization, authenticated initial-crate ownership procedure, protected publication workflow, notice/license review, actual packaged-graph SBOM and reviewed provenance/signing decisions. None is activated by technical success. Current registry trusted-publishing prerequisites and initial API-token limitation are recorded in [library packaging](library-package.md).

| Baseline binary target | Archive bytes | SHA-256 |
| --- | ---: | --- |
| `x86_64-unknown-linux-gnu` | 1840808 | `f6a8cd2275a51ade59f92a5cc5ce356a53ad35018d243e000c07f32bd7c8ec7d` |
| `aarch64-apple-darwin` | 797736 | `040de2ddbc666354023ab53b87d379858b7a18f42b9be6150b7049bbc7875913` |
| `x86_64-pc-windows-msvc` | 868275 | `a839ca5db0134d6cdc971704e291987a9e185569ae0dbc4cccf834b88cd0add4` |

These artifacts were measured and deleted, not uploaded. Native runner execution is not validation of older OS/ABI baselines, unsupported CPUs, signing/notarization, downloaded-file trust prompts or end-user installation without development tools. See the separate QA comments in #108/#109 for PR-head versus CI merge-revision measurements.

## Refreshed privacy and public-surface audit

Snapshot at the baseline above: all fetched branch refs and all 69 repository PR heads; 190 reachable commits / 667 unique blobs. Scanned current GitHub bodies for 30 issues, 69 PRs, 144 conversation comments, all 69 PR review lists and zero inline review comments; Project description/readme/items; metadata and accessible logs for all 430 listed Actions runs / 431 attempts (3690 log entries). Existing immutable completed-attempt archives were reused from the earlier source-preview audit; new/latest attempts were fetched. No unavailable log archive remained in this snapshot.

Selected signatures covered private-key headers, GitHub tokens, selected cloud/provider tokens, credential-bearing URLs and personal macOS/Windows home paths. Eight matches were reviewed: four original negative-test dummy URL occurrences on `example.invalid`, and four GitHub-hosted Windows runner short-path occurrences. None was an actual credential/personal developer path; no unresolved selected-signature finding remained. Author/committer addresses were GitHub users.noreply addresses or GitHub's web-flow noreply address. No historical Finder/AppleDouble/Thumbs path was found. 531 Markdown links were examined, with zero missing relative file/directory paths; 241 distinct external URLs were identified, not all fetched or certified. Catalog URLs received the separate bounded HEAD check above.

This is a bounded agent self-audit plus targeted document/metadata inspection, not independent review, comprehensive secret detection, semantic classification of every private sentence, or proof that secrets/private conversation content are absent. No dedicated scanner was installed. Cache metadata was inspected, not cache payloads; no cache cleanliness certification is made. Account/organization credentials and installed-app internals were not examined. Raw data/log exports remain outside the repository, and failed historical CI is retained. Do not silently delete failures or rewrite history to claim readiness. New content after this snapshot needs review; final audit-PR QA identifies its exact narrower delta and checks.

## Live settings and distinct approvals

Read-only API observations on 2026-10-17:

- Repository is **public**, following the maintainer-controlled source transition; Project #10 is **private**. Neither was changed by this audit. [Source-preview decision](../public-readiness.md) is separate from crate/binary publication.
- Main retains seven strict required Actions checks, admin enforcement, conversation resolution, linear history, no force/delete and squash-only/PR-title merging. Zero required human reviews is the explicitly authorized solo routine policy, not an independent release review.
- Workflow default token permission is read, workflow PR approval is disabled, checkout credentials are not persisted and actions are SHA-pinned. Fork PR approval policy is `first_time_contributors`; do not imply every external contributor needs approval. Review each external workflow before execution; no self-hosted runner, environment or repository Actions secret/variable was reported.
- Secret scanning and push protection are enabled. Non-provider patterns/validity checks and automatic Dependabot security updates are disabled. Private vulnerability reporting is disabled; no form/notification delivery was verified or service activated. Pages/wiki/Discussions are disabled. Zero uploaded Actions artifacts, releases and repository webhooks were reported. Metadata absence is not proof of account-wide absence.

Repository visibility is already the maintainer's source-only decision. Project publication remains unapproved and unnecessary. Crates.io and GitHub binary releases remain separately unapproved. Contribution/reporting arrangements remain deferred. Any unresolved critical safety issue blocks publication; no green-check count overrides that rule.

## Exact maintainer decisions still required

1. Independently review this candidate/history/public-facing material and the documented native/host gaps; record actual reviewer identity, revision, findings and disposition in #26.
2. Approve and verify real confidential security/conduct contacts, alternate handling and appeals before supported publication or public contribution intake. Keep them unavailable in documentation until verification succeeds.
3. If publication is later desired, choose the exact commit/version and capability/runtime boundaries; repeat final native checks, dependency/evidence/name review, package inspection and clean artifact measurements. Review the actual packaged-graph SBOM, notices and provenance/signing strategy.
4. Authorize crates.io publication and GitHub binary distribution separately, with an independently reviewed protected publishing procedure. Do not assume OIDC can create an initial crate or grant this agent token access. Do not remove publish=false, create tags/releases, upload assets or enable credentials without that authorization.
5. Decide Project visibility separately only if needed; keeping it private satisfies this plan. M5 requires a separate scope decision.

#26 stays open and M4's publication exit remains pending until its explicit human gates are satisfied. This checkpoint prepares that decision without taking it.
