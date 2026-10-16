# WIP source visibility readiness

Preparation tracked in [#93](https://github.com/wiyco/agent-skills-kit/issues/93), separately from the full release gate [ASK-026 / #26](https://github.com/wiyco/agent-skills-kit/issues/26). Decision: [ADR 0008](adr/0008-public-source-preview.md).

**WIP — not ready for use.** Public source is a development preview, not a supported/published library or CLI. Do not use it with real skill stores. No crates, installers, public binaries or releases are authorized. Public contribution intake and reporting arrangements remain deferred until a separately reviewed collaboration/release phase.

## Scope and reviewed evidence

Audit performed 2026-10-16 UTC / 2026-10-17 JST. Source base: `b24944c98ee467385984090a7959388ab05e755c`; open Crush PR #92 head `1c39ebafcefcf550d56f7ba5dc83fe7ed8c4cebe` was included without merging it. The preparation PR's QA comment identifies its own final SHA and local verification. These results do not prove later revisions.

| Surface | Evidence and result |
| --- | --- |
| Git history and code | All fetched branch refs and 55 repository PR heads; 154 reachable commits and 512 unique blobs scanned. No selected credential signature or personal home-path match. All author/committer email occurrences were GitHub noreply addresses. No historical Finder/AppleDouble/Thumbs metadata path was found. No history rewrite performed. |
| Issues and PRs | Snapshot: 29 planning Issues, 55 PR bodies, 99 issue/PR conversation comments, all 55 PR review lists and zero inline review comments. Signature/path scan plus targeted policy/privacy review. Corrected private-stage context in all 29 Issue bodies without changing progress, dependencies or historical QA failures. |
| Project | Owner Project #10 was private; description/readme/items reviewed. Updated WIP/source-versus-release wording. Workflow remains authoritative; current blocked work stays blocked. Project visibility is unchanged and public readers use repository Issues/milestones. |
| Actions logs | Downloaded all 360 listed run archives and scanned their 3,260 log entries, including terminal infrastructure failures. No selected credential signature or personal home-path match; no log archive was inaccessible in that snapshot. Logs/failures retained. |
| Artifacts and releases | Zero uploaded Actions artifacts and zero releases at review. No Pages, wiki or Discussions enabled. No public binaries or package publication added. |
| Settings metadata | Zero repository Actions secrets/variables, environments, webhooks and self-hosted runners were reported. This does not inspect account/organization-level credentials or installed-app internals. No credential values requested. |
| Build caches | 93 cache records reviewed as metadata. Workflow cache paths are build output (`target`), not home/config/credential directories. Cache payloads were not downloaded or certified free of arbitrary binary data. |
| Workflows and protection | Standard ubuntu/macos/windows runners; PR code runs under pull_request, not pull_request_target. Contents-read permissions, checkout credentials not persisted, action revisions pinned, untrusted title passed through an environment variable. Seven strict required checks, admin enforcement, conversation resolution, linear history, no force/delete and squash-only merging retained. |
| License and capability claims | MIT OR Apache-2.0 original-project notices and third-party attribution retained. No license or safety guarantees invented. Matrix separates documented/resolver evidence from unrun real-host discovery. M0–M2 complete; M3/M4 remain incomplete. No release-candidate claim. |

The scan used bounded credential signatures (private-key headers, GitHub tokens, selected cloud/provider credentials and credential-bearing URLs) and personal Unix/Windows home-path patterns. It was combined with targeted inspection of policy, workflow, metadata and current-claim findings. Dedicated secret scanners were not installed. This is a maintainer-assisted audit, not independent review, exhaustive personal-data classification or proof of secret absence; transformed/unknown credentials and unrecognized private prose can evade these checks. Raw API/log exports remain outside the repository and are not committed. Refresh this audit for new content before visibility changes.

## Changes prepared for public reading

- README and API/CLI examples explicitly say WIP, unsupported and not ready for use. Development inspection commands do not install skills.
- Contribution, security and conduct documents stop presenting repository Issues as a confidential reporting channel. No unverified address, alternate person or reporting service is advertised. ADR 0008 records the maintainer's explicit deferral for this source-only preview.
- Issue templates warn against public sensitive/exploit reports and link to the actual security policy rather than an unenabled reporting form.
- Planning documents and live Issue context separate source visibility from crates/binaries. The private-only provisioning tool remains private-only; do not rerun it against a public repository.
- Historical implementation approvals, dated bootstrap evidence and failed CI results remain factual records, not current readiness promises.

## Remaining prerequisite: CI and merge

**Do not change visibility on the assumption that the preparation is already on main.** Required CI for #92 and this preparation is blocked by GitHub's runner-start restriction. Provider annotations identify account billing/spending configuration; no account payment details were collected. Jobs that did not start are unrun, not passing code tests. Local verification and earlier green commits cannot satisfy final-revision native CI.

Ordinary PRs must pass the existing checks before merging. The maintainer subsequently authorized the one-time preparation exception below; it is not inferred from public runner pricing. No direct push, fabricated status, alternate default branch, billing change, purchase or automatic visibility action is authorized. Account administration/support may still be needed if public runner availability does not recover.

## Explicit one-time preparation merge exception

After reviewing the blocked preparation, the maintainer explicitly authorized temporary protection relaxation for PR #94 only. Record the exact final SHA and the local-versus-unrun CI evidence in its QA comment. Temporarily disable admin enforcement only, use a SHA-matched administrative squash merge, and immediately restore and compare the saved protection settings even if the merge fails. Required checks remain configured for ordinary contributors throughout. No other PR, including #92, receives this exception.

This supersedes the ordinary green-CI prerequisite solely for this source-preview preparation merge. It is not a passing CI result, independent approval or a library/CLI release authorization. All seven checks, admin enforcement and other protections must be restored before the handoff is complete. Post-transition native CI remains unverified until it actually runs.

## Maintainer handoff

Before changing repository visibility personally:

1. Review the preparation PR and its final-SHA evidence, confirm its explicit one-time exception and restored protections, and confirm it is on main. #92 can remain open until its own exact-revision gates pass; do not advertise its unmerged adapter as shipped.
2. Confirm the intended public scope is WIP source/history/Issues/PRs/Actions logs, including existing generic infrastructure-failure annotations. Review any content added since the audit. Project #10 stays private.
3. Confirm no package/binary publishing or release workflow was added and `publish = false` remains. Reporting/contribution arrangements are deferred, not operational or silently approved for release.
4. Use GitHub's repository visibility settings yourself. This document and the agent do not execute that action.

Immediately after the maintainer's visibility change, before approving external workflows or opening contribution intake:

1. Verify source/Issues/log visibility and that the Project remains private. Recheck main protections, seven required checks and squash-only settings; GitHub documents visibility-change effects, so do not assume settings survived unchanged.
2. Review fork workflow approvals, restrict untrusted external workflow execution appropriately, retain read-only tokens and never expose secrets/self-hosted runners to untrusted PRs. The approval-policy API returned 422 while private; its public setting is not verified here.
3. Optionally enable GitHub private vulnerability reporting and verify the form plus notification delivery, then update SECURITY.md truthfully. The private-stage API returned 404; no working public form is claimed. Actual reporting/alternate/appeal arrangements and contributor intake require the deferred maintainer decisions before supported publication or public collaboration.
4. Rerun final required CI at unchanged PR heads and confirm the account restriction is actually cleared. Public standard runners are free under the cited model; larger runners and other paid services are separate, and an account-wide restriction may still need GitHub intervention.
5. Resume authorized M3/M4 implementation only after its gates are available. Keep crates.io publication, binary release, Project visibility and M5 pending separate authorization.

## Primary references

Reviewed 2026-10-16 UTC:

- [GitHub Actions billing](https://docs.github.com/en/billing/concepts/product-billing/github-actions): public repositories using standard GitHub-hosted runners are free; this is not a guarantee of universal zero charges or account recovery.
- [Repository visibility effects](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/managing-repository-settings/setting-repository-visibility): public source and Actions history/log exposure; assess settings after transition.
- [Project visibility](https://docs.github.com/en/issues/planning-and-tracking-with-projects/managing-your-project/managing-visibility-of-your-projects): independent from repository access and visibility.
- [Private vulnerability reporting](https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/configure-vulnerability-reporting/configure-for-a-repository): public-repository enablement, reporting form and notification configuration.
- [Actions secure use](https://docs.github.com/en/actions/reference/security/secure-use): least privilege, untrusted code, script injection and limits of automatic log redaction.
