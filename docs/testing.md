# Test strategy

## Isolation is mandatory

All writes go under test-owned temporary roots. Inject home, project roots, OS/environment inputs, and relevant host overrides. Never write to the real home or installed agent stores. Avoid process-global environment changes in parallel tests. Do not execute SKILL.md instructions or bundled scripts.

## Layers

1. Pure catalog/schema and path-rule tests: no filesystem changes.
2. Validator/inventory tests: bounded reads of original inert fixtures.
3. Planner tests: prove that the tree is unchanged after planning.
4. Executor integration tests: native OS behavior with test-owned roots and fault injection.
5. CLI integration tests: flags, rendering, JSON schema, exit codes, and TTY/non-TTY behavior.
6. Optional real-host discovery tests: isolated config/project, recorded tool version and OS, no paid/credentialed calls without approval.

## Required matrix

| Concern | Minimum cases | Evidence |
| --- | --- | --- |
| Roots and scope | explicit project, missing project, user home, application workspace | typed results and no guessed roots |
| Paths | spaces, Unicode, non-UTF-8 where supported, relative components, Windows drive/UNC | native OS tests |
| Names | valid, traversal, absolute, separators, reserved device names, case collisions | no escape or collision |
| Input format | missing SKILL.md, invalid YAML, missing required metadata, extension keys, BOM/CRLF policy | bounded diagnostics |
| File types | normal files, executable bit, symlink, junction/reparse point, FIFO/device | documented rejection or supported handling |
| Containment | source equals/contains destination, destination contains source, overlapping targets | plan rejection before writes |
| Existing content | absent, identical, different, unmanaged, manually modified | create/unchanged/conflict |
| Concurrency | source changes, destination appears, parent changes, two installers | stale-plan/race protection |
| Faults | read/copy/permission/rename failure, interruption, cleanup failure | existing content preserved |
| Multi-target | deduplicated destinations, earlier success plus later failure | complete outcome report; no global-transaction claim |
| Permissions | read-only roots, ordinary execute bits, privilege bits | no unexpected escalation |
| CLI | non-TTY, --yes, cancellation, invalid flags, stdout/stderr separation | no hang and no force semantics |
| Evidence | stale docs, conflicting sources, missing platform details | unsupported/unverified remains explicit |

Run filesystem behavior on Linux, macOS, and Windows. Testing Windows-looking strings on Unix is not a substitute. Platform limitations (for example, link-creation permissions) require an explicit skip reason and cannot count as proof of support.

## Safety properties

Planning has no writes. Execution does not follow untrusted descendants outside approved roots. Different existing content is not clobbered. Cleanup only removes operation-owned paths after identity checks. Unchanged installation is idempotent. A stale plan does not become an authorization for a different operation.

Use deterministic hooks for races/fault injection rather than timing-only sleeps. Use property tests where they improve confidence in naming, rule parsing, containment, and stable ordering. Record seeds for failures.

## Reporting

PRs state the commands actually run and their results. Report untested platforms, scopes, host versions, and failure modes explicitly. Documentation review, resolver tests, and real-host discovery tests are separate columns in the support matrix.

An MVP security regression blocks release. Broad catalog expansion does not excuse missing coverage for already shipped targets.
