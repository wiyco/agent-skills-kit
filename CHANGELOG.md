# Changelog

## Unreleased

This is pre-publication work toward a 0.1 candidate. No release, crate publication,
public binary distribution or tag is implied by the development version.

- Library-first bundled catalog with rule-specific official-source evidence,
  explicit environment/project contexts and separate resolver/host observations.
- Bounded inert SKILL.md validation and inventory, read-only deterministic plans,
  retained native copy backends, no-clobber commits and explicit partial outcomes.
- Schema-versioned lossless receipts and an external Rust consumer example.
- Thin inspection/planning/install CLI with explicit approval, non-TTY behavior
  and opt-in interactive selection; no force overwrite or remote acquisition.
- Native Linux/macOS/Windows installer tests, measured MSRV checks and documented
  filesystem/host limits; discovery success is recorded only where actually run.
- Evidence drift review, isolated opt-in smoke procedures and dependency/license
  policy checks. Crate and binary rehearsals remain separate from publication.

Before publication, review the exact candidate diff/API and update this entry with
the approved version/date. For later 0.x releases, document breaking public API,
receipt-schema, catalog-semantic or CLI-contract changes explicitly and select a
new minor version; use patch releases for compatible corrections. Do not silently
reuse a schema version or infer compatibility from a Conventional Commit title.
