# Contributing

**WIP: public contribution intake and user support are not open.** Existing authorized maintainer development follows this guide. The public source preview is not a usable supported library or CLI, and it must not be used with real skill stores.

This is unreleased development. Source visibility, contribution readiness and crate/binary publication are separate decisions. Original contributions follow the approved MIT OR Apache-2.0 licensing and attribution policy; there is no released stability or support guarantee. See the [source visibility checklist](docs/public-readiness.md) for outstanding transition gates.

Read [AGENTS.md](AGENTS.md) for implementation rules; they apply to human-assisted and automated changes alike. Use one issue and one focused PR per coherent change. All repository-facing writing is in English.

## Definition of ready

The issue has a concrete goal, bounded scope, acceptance criteria, tests, and satisfied dependencies. Target-adapter issues also identify official documentation and unresolved evidence gaps. Legal, naming, or publication decisions require the maintainer rather than an automated assumption.

## Definition of done

The implementation is reviewed and merged, acceptance criteria are satisfied, required CI is green, documentation is updated, and test limitations are disclosed. An issue is not complete merely because code was generated.

## Target adapters

Include the product/variant, supported scopes and platforms, preferred destination versus other discovery paths, documented overrides, relevant reserved names, source URL/section/access date, and resolver tests. Real-host discovery tests are separate evidence. Never advertise research candidates as supported.

## Changes to the public API

Explain consumer impact, error/report compatibility, feature policy, and MSRV effects. Prefer additive changes. Record architectural or safety tradeoffs in an ADR. The CLI must exercise the same API exposed to other Rust applications.

## Dependencies and attribution

Use the smallest justified dependency set. Review maintenance, license, and MSRV implications. Follow the [attribution policy](docs/attribution.md) before copying third-party code, catalog data, documentation, or fixtures. Original small fixtures are preferred.

## Pull requests

Use the PR template, link the actual issue number, and report the exact checks run. Commit subjects and PR titles use Conventional Commits. Keep secrets, private user paths, conversation details, and unrelated changes out of commits and logs. Routine squash merges require every safeguard in the [autonomous development policy](docs/autonomous-development.md); explicit task-specific approval gates remain binding. Publication requires separate approval.

## Reporting concerns

Follow the [code of conduct](CODE_OF_CONDUCT.md). Security issues must not be posted as public exploit reports; follow [SECURITY.md](SECURITY.md). Issues, PRs and reviews are not confidential reporting channels. If a policy marks its confidential or alternate reporting route as pending, do not send sensitive details there or assume GitHub reporting is enabled. The maintainer has deferred those arrangements for the WIP source preview; they must be approved and verified before public contribution intake or supported library/CLI publication.

## Repository hygiene

Do not commit operating-system metadata (`.DS_Store`, `._*`, `Thumbs.db`), editor scratch files, credentials, local configuration, or generated build artifacts. `.gitignore` is a convenience for ordinary Git staging, not a security boundary; APIs that write Git trees directly can bypass it. Before proposing public visibility or releases, audit `git ls-files`, the **full** commit history, and any archives and release artifacts for accidental additions.
