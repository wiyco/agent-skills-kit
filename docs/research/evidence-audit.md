# Offline catalog evidence audit

This maintainer tool is separate from the product's library/CLI and performs no network access. It consumes an explicit JSON export of the validated catalog, groups canonical products/variants and aliases, deduplicates identical source URLs while retaining each preferred/read rule's evidence, and reports age/missing-evidence findings. It never reads or runs skills, computes an installation path, rewrites an adapter, or discovers a host environment.

Use the committed Rust toolchain and the existing Python 3 interpreter; no Python package installation is required. The local verification environment for this implementation used Python 3.15.0; GitHub CI records its runner interpreter separately. Other interpreter/platform combinations are not implied validated. Output files below are task-owned temporary files, not real skill directories:

```sh
audit_tmp=$(mktemp -d)
cargo run --quiet -p agent-skills-kit --example catalog_json --locked > "$audit_tmp/catalog.json"
python3 -B .github/scripts/catalog_drift.py --catalog "$audit_tmp/catalog.json" --as-of 2026-10-16 > "$audit_tmp/evidence-report.json"
python3 -B -m unittest discover -s .github/scripts/tests -v
```

Keep generated reports in that task-owned temporary directory. The exporter uses the public library facade and retains core rules generated in Rust as well as expanded JSON entries; the audit does not parse Rust source or maintain a second registry. Export is a development example, not a library/CLI publication.

## Report interpretation

Report version 1 explicitly states `path_correctness: not_assessed`, human review before catalog changes, and `probe.state: not_run`. Recorded resolver/discovery references are metadata, not tests run by the command. No support count or discovery success is inferred from a recent date, an alias, a shared URL or an installed file.

The default stale threshold is 90 days: age 89 is within the window, age 90 is stale. Override with `--stale-after-days` from 1 to 3650. Missing, invalid and future access dates are separate findings; future dates never count as recent review. Dates use strict YYYY-MM-DD and the default as-of date is UTC. Supplying `--as-of` makes review reproduction deterministic.

Canonical product grouping retains CLI/IDE/cloud variants and their scope statuses. Aliases do not become new products. A fork with different documented behavior remains a distinct identity; this tool does not infer equivalence from ancestry, branding or matching directories. The enhanced target proposal template requires official identity/rename/fork evidence and links to existing proposals. Import-only, unsupported and configuration-required states are reported separately from documented ones, without guessing missing paths. Targets lacking scope evidence produce `no_scope_evidence`; scopes lacking rule evidence produce a tracked `no_rule_evidence` finding, even when that absence is expected for an unsupported surface.

URL groups preserve separate per-rule dates and evidence; one recent rule cannot refresh an older rule that cites the same page. Missing source/URL/section/applicability, unreviewed documentation and malformed recorded test references produce findings. Malformed/non-HTTPS or credential-bearing URLs produce a gap without echoing the offending URL. This is syntax handling, not proof of host ownership or content correctness.

Input is bounded to 2 MiB, 512 target records and 4096 rule references. Invalid shape, schema version, duplicate IDs/aliases/product-variants/JSON keys or invalid command parameters fail with exit 2; a valid report with findings exits 0. Findings are review work, not an automated approval or automatic edit gate. The Rust catalog validator remains authoritative for schema/path safety; the Python audit intentionally does not implement it again.

## Verification boundary and next layer

Original synthetic unit fixtures cover stale/future/leap/invalid dates, shared-URL old/new evidence, canonical aliases and distinct variants/forks, unsupported/import-only statuses, missing sources, credential redaction, malformed JSON/identity/rule/input limits, deterministic ordering, no sockets and read-only CLI behavior. Tests use in-memory records and owned temporary files only. The existing required quality job runs these offline tests.

The dependent ASK-021 layer will add opt-in bounded online redirect/error observations, a manual read-only CI report and a documented 180-day candidate review. Those checks are not implemented or claimed by this foundation. Online transport observations must stay separate from offline tests and never become path correctness or adapter updates. No fetched page or skill content may be executed, and report-driven catalog changes require human review.
