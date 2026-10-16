## Summary

Use a Conventional Commits PR title (`type(scope): imperative summary`).

Closes #<actual-issue-number> (or `Refs #N` for an intermediate stacked PR)
Plan key: ASK-___
Stack dependency: <link to lower PR, or none>

## Changes and scope

Describe behavior/API changes and what is intentionally not included.

## Verification

List exact commands actually run and results. Identify untested platforms, scopes, and host versions.

## Target evidence (when applicable)

Official URL, section, access date, product/version, path/override rules, and resolver/discovery test coverage.

## Safety review

Explain filesystem containment, existing-content handling, races, links/reparse points, failure cleanup, and trust implications relevant to this change.

## Self-review and QA handoff

Describe the final-diff review, affected invariants, unresolved risks, and whether dedicated QA is required. For QA-sensitive changes, add a **separate PR conversation comment** titled `QA Evidence` after the final push, including exact SHA, environment, scenarios, commands, observed outcomes, and CI run links; reference that comment here.

## Checklist

- [ ] Acceptance criteria are satisfied or remaining blockers are explicit.
- [ ] Tests use temporary roots, not real user/agent directories.
- [ ] No skill scripts or embedded instructions were executed.
- [ ] Documentation and changelog/ADR are updated where applicable.
- [ ] No unverified paths, versions, test results, or release claims are introduced.
- [ ] The commit subjects and PR title follow Conventional Commits.
- [ ] Final diff was self-reviewed, mandatory CI is green for the exact revision, and stack dependencies (if any) are also green.
- [ ] Dedicated QA evidence was posted in a PR comment when required; gaps are explicit.
- [ ] No publication or visibility change is performed without separate approval.
