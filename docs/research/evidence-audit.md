# Catalog evidence audit and explicit discovery review

This maintainer tool is separate from the product's library/CLI and defaults to no network access. It consumes an explicit JSON export of the validated catalog, groups canonical products/variants and aliases, deduplicates identical source URLs while retaining each preferred/read rule's evidence, and reports age/missing-evidence findings. It never reads or runs skills, computes an installation path, rewrites an adapter, or discovers a host environment.

Use the committed Rust toolchain and the existing Python 3 interpreter; no Python package installation is required. The local verification environment for this implementation used Python 3.15.0; GitHub CI records its runner interpreter separately. Other interpreter/platform combinations are not implied validated. Output files below are task-owned temporary files, not real skill directories:

```sh
audit_tmp=$(mktemp -d)
cargo run --quiet -p agent-skills-kit --example catalog_json --locked > "$audit_tmp/catalog.json"
python3 -B .github/scripts/catalog_drift.py --catalog "$audit_tmp/catalog.json" --as-of 2026-10-16 > "$audit_tmp/evidence-report.json"
python3 -B -m unittest discover -s .github/scripts/tests -v
```

Keep generated reports in that task-owned temporary directory. The exporter uses the public library facade and retains core rules generated in Rust as well as expanded JSON entries; the audit does not parse Rust source or maintain a second registry. Export is a development example, not a library/CLI publication.

## Report interpretation

Offline report version 1 explicitly states `path_correctness: not_assessed`, human review before catalog changes, and `probe.state: not_run`. Recorded resolver/discovery references are metadata, not tests run by the command. No support count or discovery success is inferred from a recent date, an alias, a shared URL or an installed file.

The default stale threshold is 90 days: age 89 is within the window, age 90 is stale. Override with `--stale-after-days` from 1 to 3650. Missing, invalid and future access dates are separate findings; future dates never count as recent review. Dates use strict YYYY-MM-DD and the default as-of date is UTC. Supplying `--as-of` makes review reproduction deterministic.

Canonical product grouping retains CLI/IDE/cloud variants and their scope statuses. Aliases do not become new products. A fork with different documented behavior remains a distinct identity; this tool does not infer equivalence from ancestry, branding or matching directories. The enhanced target proposal template requires official identity/rename/fork evidence and links to existing proposals. Import-only, unsupported and configuration-required states are reported separately from documented ones, without guessing missing paths. Targets lacking scope evidence produce `no_scope_evidence`; scopes lacking rule evidence produce a tracked `no_rule_evidence` finding, even when that absence is expected for an unsupported surface.

URL groups preserve separate per-rule dates and evidence; one recent rule cannot refresh an older rule that cites the same page. Missing source/URL/section/applicability, unreviewed documentation and malformed recorded test references produce findings. Malformed/non-HTTPS or credential-bearing URLs produce a gap without echoing the offending URL. This is syntax handling, not proof of host ownership or content correctness.

Input is bounded to 2 MiB, 512 target records and 4096 rule references. Invalid shape, schema version, duplicate IDs/aliases/product-variants/JSON keys, non-finite JSON numbers or invalid command parameters fail with exit 2; a valid report with findings exits 0. Findings are review work, not an automated approval or automatic edit gate. The Rust catalog validator remains authoritative for schema/path safety; the Python audit intentionally does not implement it again.

## Opt-in online observations

Add `--online` to the audit command to issue HTTPS HEAD requests. This changes only the generated report, never the input, rules, evidence access dates or recorded tests. There is no GET fallback, response-body read, page interpretation, skill download or execution. HTTP 403/405/429 and transport errors remain visible findings requiring manual investigation; successful responses do not establish correct paths, ownership, applicability or host discovery.

The observer allows port 443, verified TLS, at most 64 distinct fragment-stripped URLs, five redirects per URL and four address attempts per request. It refuses credentials, local/nonpublic literals, mixed public/private DNS answers, non-ASCII URL spellings and unsafe redirects. Connections use an already validated public address with the original TLS hostname, avoiding a second DNS lookup. Redirect-chain URLs omit query strings and fragments; raw exception/unsafe redirect values are not echoed. These controls are a conservative maintenance-tool boundary, not a general network sandbox.

`--timeout-seconds` defaults to 8 (maximum 30), and `--budget-seconds` defaults to 180 (maximum 300). The budget is checked between requests; DNS resolution and individual socket/TLS/header operations can exceed that wall-clock budget. It is not a hard global deadline. No environment proxy, cookies, authentication or netrc credentials are used. Run only against reviewed public documentation metadata.

`.github/workflows/catalog-evidence.yml` is **manual only** (`workflow_dispatch`), uses a read-only repository token and a ten-minute job timeout, and writes an HTML-escaped job summary. It does not upload artifacts, modify the catalog, open PRs/issues or run on untrusted PRs. After the workflow is available on main, maintainers can explicitly run it:

```sh
gh workflow run catalog-evidence.yml --ref main
```

Inspect stale/missing evidence and every non-reachable observation. Review redirected content manually before updating a canonical source. Record the official section, access date, version/scope and uncertainty in a focused proposal/PR. A report never refreshes evidence or authorizes an adapter change; a human must review report-driven catalog changes.

## Discovery intake

For each explicit discovery task, record the latest 180-day inclusive window, then inspect adjacent Warp, Augment, Continue, Replit and TRAE surfaces even when their initial announcements predate it. Use the target-support issue template. Record discovery and actual event dates separately, allowing unknown launch dates. Search official announcements, release notes, documentation and repositories; third-party catalogs provide leads only. Deduplicate renamed products and aliases, but retain differing variants/forks as separate behaviors. Classify hosted/import-only/unsupported surfaces explicitly instead of inventing local paths.

The [2026-10-16 bounded review](discovery-2026-10-16.md) records primary sources, dated changes and tracked adoption gaps. This is a reproducible review sample, not a claim to enumerate all products or launches.

## Verification boundary

Original synthetic unit fixtures cover stale/future/leap/invalid dates, shared-URL old/new evidence, canonical aliases and distinct variants/forks, unsupported/import-only statuses, missing sources, credential redaction, malformed JSON/identity/rule/input limits, deterministic ordering, no sockets and read-only CLI behavior. Tests use in-memory records and owned temporary files only. The existing required quality job runs these offline tests.

Additional offline transport fixtures cover relative/cross-host redirects, loops, missing Location, hop limits, HTTP errors including HEAD refusal, timeout redaction, malformed/private URLs, mixed/private DNS rejection before socket creation, pinned public connections with TLS hostname and HEAD-only requests, fragment deduplication, request budgets and input preservation. They inject transport responses; no public network is needed. Online runs are separately recorded observations. None proves the documentation's contents remain correct or validates an actual host.
