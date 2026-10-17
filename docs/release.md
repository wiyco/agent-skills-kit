# Release and OSS publication gate

This document is a checklist, not authorization to publish. The [candidate-specific technical checkpoint](verification/release-readiness.md) records completed implementation, measured CI/package/binary evidence and remaining human decisions.

Source visibility can precede completion of M3/M4, provided unfinished capabilities and release gates remain explicit. [Source visibility readiness](public-readiness.md) records the separate preparation under #93. It does not complete ASK-026, authorize package/binary publication, or waive CI/merge safeguards.

## Before any visibility change

- Confirm final names and the approved license, attribution, and contributor policy.
- Review tracked files and full Git history, issues, PRs, Projects text, logs, artifacts, and links for secrets/private information.
- For supported use, public contribution intake or release, verify confidential security and conduct reporting, alternate handling and appeal routes. The maintainer explicitly deferred these for the WIP source-only preview in [ADR 0008](adr/0008-public-source-preview.md); do not advertise unavailable routes or open contribution intake.
- Confirm README and examples describe features that actually exist, with no fabricated badges or releases.
- Review documentation, resolver tests, and real-host evidence for every advertised target.

## Before the first crate

- Verify crate-name availability again; a repository name is not a crates.io reservation.
- Confirm a measured MSRV, green native CI, API docs, and an external consumer test.
- Inspect packaged files and ensure catalog assets are included and local data excluded.
- Review dependency advisories, licenses, and exceptions.
- Rehearse packaging in an isolated copy. `publish = false` stays in the development workspace until approved.
- Follow the [unpublished library rehearsal](verification/library-package.md); inspect the actual archive and test an independent consumer against its unpacked contents. A successful rehearsal does not authorize the upload step.
- Verify the current registry authentication/publishing mechanism from official documentation. Do not assume trusted publishing supports every initial-publication case.
- Approve the exact release commit/version and the dependency order for the library and CLI crates.

## Before CLI binary distribution

- Review tested OS/architecture/ABI targets, checksums, provenance expectations, and installation instructions.
- Test --help and --version on the declared native platforms using the [unpublished native binary rehearsal](verification/binary-rehearsal.md).
- Keep upload/publish jobs approval-gated; no public release URL exists until an actual release succeeds.

## Separate approvals

The maintainer explicitly approves each of: repository visibility change, Project visibility change (optional), crates.io publication, and GitHub binary release. Do not couple these into a background workflow. The Project can stay private when the code is made public, subject to the intended information-sharing policy.

ASK-026 records approval or outstanding blockers. Post-MVP features and every catalog candidate need not be complete, but the shipped capability matrix must be accurate.

## Experimental CLI alpha preparation

The separately authorized CLI-only `0.1.0-alpha.1` preparation is tracked by #114 and [its candidate/export checklist](verification/cli-alpha.md). Keep the embedded library unpublished, verify actual packaged list/copy/permanent-delete flows on all three native ABIs and use a fresh manual build. The alpha scope does not silently resolve confidential reporting, alternate/appeal contacts or independent-review gates above; record the maintainer's resolution for the concrete candidate before public artifact export/release. Full supported publication and crates.io remain separate decisions under ASK-026.
