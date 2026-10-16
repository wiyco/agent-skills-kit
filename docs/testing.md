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

## Reproducible seeded properties

`cargo test -p agent-skills-kit --test properties --locked -- --nocapture` runs original bounded generators with seeds `0x1`, `0x5eed`, `0xc0decafe` and `0x123456789abcdef0`. A failure prints `PROPERTY COUNTEREXAMPLE` with the seed/case before resuming the original assertion, so rerunning the exact named test reproduces the same generated input. These are sampled invariants, not exhaustive proofs or cryptographic randomness; no new testing dependency or mutable process-global configuration is required.

- `seeded_skill_name_properties_reject_unsafe_mutations`: 64 cases per seed, constructive Unicode/lowercase names and unsafe traversal/separator/stream/case/delimiter/control mutations, parsed only as inert metadata.
- `seeded_native_rule_properties_preserve_lexical_containment_and_explicit_overrides`: 64 cases per seed using original synthetic catalog data, native absolute roots, aliases, explicit override/fallback choices and rejected unsafe components. No actual host path rule or filesystem operation is introduced.
- `seeded_receipt_properties_round_trip_native_units_and_outcome_states`: 64 cases per seed across preview/outcome combinations, ordinary modes, variable entries and non-Unicode native units; stable JSON round trips and unknown-version refusal. Synthetic digests describe codec data, not real file contents. Windows exercises its own native string family.
- `native::seeded_planning_properties_are_read_only_and_permutation_invariant`: 16 cases per seed on Linux/macOS, two owned source fixtures, shuffled/duplicate core target selections and source order, explicit user/project contexts with spaces/Unicode, identical semantic plans and unchanged before/after byte/mode trees. Windows installation planning remains unsupported under ADR 0006; its native resolver/codec properties are separate from successful copy coverage.

Map relevant layer tests to [threat scenarios TM-01 through TM-22](threat-model.md#attack-and-failure-scenarios-for-ask-012). A documentation scenario is not an executed test. Backend support cannot be advertised until native evidence covers its promised conditions; unsupported-case rejection is separate from successful installation coverage.

## Reporting

PRs state the commands actually run and their results. Report untested platforms, scopes, host versions, and failure modes explicitly. Documentation review, resolver tests, and real-host discovery tests are separate columns in the support matrix.

An MVP security regression blocks release. Broad catalog expansion does not excuse missing coverage for already shipped targets.
