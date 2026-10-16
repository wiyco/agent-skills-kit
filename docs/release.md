# Release and OSS publication gate

This document is a checklist, not authorization to publish.

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
- Verify the current registry authentication/publishing mechanism from official documentation. Do not assume trusted publishing supports every initial-publication case.
- Approve the exact release commit/version and the dependency order for the library and CLI crates.

## Before CLI binary distribution

- Review tested OS/architecture/ABI targets, checksums, provenance expectations, and installation instructions.
- Test --help and --version on the declared native platforms.
- Keep upload/publish jobs approval-gated; no public release URL exists until an actual release succeeds.

## Separate approvals

The maintainer explicitly approves each of: repository visibility change, Project visibility change (optional), crates.io publication, and GitHub binary release. Do not couple these into a background workflow. The Project can stay private when the code is made public, subject to the intended information-sharing policy.

ASK-026 records approval or outstanding blockers. Post-MVP features and every catalog candidate need not be complete, but the shipped capability matrix must be accurate.
