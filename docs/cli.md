# Command-line usage

The executable is `agent-skills-kit`; it consumes the same library catalog and validation APIs as other Rust applications. Commands use the library for inspection, planning and approved copying:

```text
agent-skills-kit targets list [--json]
agent-skills-kit targets show <id> [--json]
agent-skills-kit validate <source> [--json]
agent-skills-kit plan <source> --target <id>... --scope <user|project> [context] [--json]
agent-skills-kit install <source> --target <id>... --scope <user|project> [context] [--yes] [--dry-run] [--json]
agent-skills-kit --help
agent-skills-kit --version
```

No arguments or `--help`/`-h` displays help. `--version`/`-V` must be used alone. `--json` is a global flag accepted once before the `--` terminator; use `--` before a positional path/ID beginning with a dash. Target IDs/aliases are case-sensitive UTF-8; filesystem source paths stay native. Relative sources are joined to the CLI working directory explicitly before calling the library; parent-traversal inputs remain subject to native library refusal. The library itself never discovers or changes CWD. Missing/extra arguments, unknown flags/targets and repeated JSON flags are usage errors. Help takes precedence when encountered; the non-interactive commands never read redirected stdin, prompt, or wait for a TTY.

`targets list` returns canonical definitions in library order; `targets show` accepts a canonical ID or explicit alias. Definitions show evidence/scopes/preferred writes and discovery locations. Choosing a tool is placement, not exclusive visibility: other hosts may read compatible locations. Listing a documented path is not proof of host discovery or safety on every deployment.

`validate` performs the library's bounded inert source inventory, including metadata/resource validation, unsafe entry refusal and native filesystem capability checks. It never creates destinations or executes skill content. The human result names the skill and inspected entry/byte counts. Validation is not malware review. Actual native eligibility and limitations are in [validation](validation.md) and [execution](execution.md).

## Read-only plans

`plan` accepts repeated `--target <id>` selections and requires `--scope user` or `--scope project`. Supply `--user-home <path>` for user scope or `--project-root <path>` for project scope. Both roots may be supplied as explicit context, but no HOME, USERPROFILE or host override is inferred. No advanced path override is included. Relative roots/source paths are joined to the CLI working directory; parent components are preserved and refused by the library rather than canonicalized across possible links. A missing destination root can be observed without being created.

The command calls `inventory_skill` and `plan_install` only. It renders every physical destination and all problems, canonical target/source attribution, intended writes, and source/target metadata through public accessors. Repeated selections/aliases are deduplicated by the library, not by a second CLI registry. An applicable preview is not proof of write permission or an approval to apply it. No staging, receipts, environment changes or destination writes occur.

Human output includes the library's cross-host/untested-discovery limitations and destination actions. JSON `plan` data contains `scope`, `applicable`, `sources`, `targets`, `destinations`, `intended_writes` and `inspected_bytes`. Sources include lossless native `path`, name/count/bytes. Targets include canonical IDs, preferred-write/discovery paths, warning text and optional resolution diagnostics. Destinations include native path/null, action (`create`, `unchanged`, `conflict`, `unsupported`), source/target index attribution and diagnostic strings. Intended writes include native path, logical kind, ordinary mode intent, byte length and optional BLAKE3-256 digest; mode is not a Windows ACL equivalence. Diagnostic prose/Debug representations are not stable error codes. An inapplicable plan retains this data with envelope `status: "blocked"` and exit 2. This does not turn a conflict into replacement authority.

## Approved installation and dry-run

`install` uses the same required source/target/scope/context options and immutable library plan as `plan`. Without `--yes`, an applicable plan is returned with exit 3 and `status: "approval_required"`; no writes are attempted and stdin is never read, including on a TTY. `--yes` explicitly approves the plan derived by this invocation. A previous command's preview/receipt is not an executable approval token: every invocation derives/revalidates a fresh plan. It never permits force overwrite, conflict adoption, broader permissions or a native capability fallback. Duplicate approval flags and `--force` are errors.

`--dry-run` returns the same plan with no apply call, staging, receipt persistence or writes. It needs no `--yes`; supplying both remains read-only. Inapplicable plans exit 2 with their diagnostics before any apply call. Applicable approved plans use `apply_plan` with its default stop-on-failure policy; the CLI adds no installation or cleanup engine. Earlier successes survive later failures. No receipt is persisted automatically.

Human output includes derived placement/warnings and every outcome. Structured `install` data contains `plan`, `dry_run`, `approval_required`, `apply_attempted`, `report`, `receipt`, `receipt_error`. Before apply, report/receipt are null. After apply, report includes `successful`, optional preflight diagnostic, all physical `outcomes` and `cleanup_errors`. Each outcome has native `path`, source/target attribution, status (`created`, `unchanged`, `failed`, `committed_with_permission_failure`, `not_attempted`), `payload_was_committed` and optional error prose. Diagnostics are not stable machine error categories. Unexpected native path representation failure uses an explicit `encoding_error`/debug label instead of silently dropping an outcome; consumers must not interpret that fallback as a native path.

A bounded schema-1 library receipt is included when construction/encoding succeeds. `cli-install` is a correlation label, not a unique ownership proof. Receipt failure keeps the complete runtime report and returns exit 4; it never rolls back or hides committed destinations. Cleanup uncertainty also prevents overall success. Committed permission failure means payload data exists with failed/partial permissions; unchanged retries do not repair it or gain orphan cleanup authority. Inspect outcomes and [recovery limits](execution.md#cleanup-authority-and-recovery) before retrying. A failed/closed output pipe may occur after commits, returning exit 1 without a complete document; missing output never proves nothing was installed.

Exit 4/`status: "failed"` means an attempted apply or receipt/report completion needs attention, with per-target results retained. Human per-target failures are part of the stdout outcome report; pre-command/usage errors still go to stderr. No cross-target transaction, rollback, real-host discovery, power-loss durability or permission-recovery promise is introduced. Interactive selection/confirmation remains ASK-015 work.

## Output schema and exit codes

Without `--json`, results go to stdout and errors to stderr. Diagnostics escape terminal controls/directional formatting; untrusted error newlines/tabs are rendered as text on one line. With `--json`, exactly one compact JSON document plus newline goes to stdout, and normal diagnostic stderr is empty. No ANSI styling is emitted. A broken output pipe is an I/O failure and may prevent a complete document; the process exits 1 instead of panicking.

Successful envelope schema 1:

```json
{"schema_version":1,"command":"targets.list","status":"ok","data":{"schema_version":1,"targets":[]}}
```

`data` depends on `command`: `targets.list` is the library's catalog document; `targets.show` is one canonical target definition; `validate` contains `source`, `name`, `description`, `resource_entries`, `total_bytes`; `help` contains `usage`. `source` is the library's lossless tagged [native-path representation](receipts.md#lossless-native-paths), never a lossy display path. Nested catalog schema versions remain independent from this CLI envelope version. Data remains untrusted and observational.

Errors use `command: "error"`, `status: "error"` and `error: {"exit_code": 2, "message": "..."}` instead of `data`. Messages are human diagnostics, not stable parsable error categories. JSON syntax escapes C0, C1 and directional formatting characters while preserving their original values when decoded; machine consumers must still treat decoded strings as untrusted. No literal terminal escape/control sequence is emitted.

Exit 0 means success/help/version; 1 means validation/native/serialization/output I/O failure; 2 means invalid command/options/unknown target or an inapplicable plan (whose structured data is retained). No mutation or overwrite approval is implied by any exit code. Exit 3 is approval required without writes; exit 4 is a failed/partial apply or receipt completion (preserve its report).

## Verification and dependencies

`cargo test -p agent-skills-kit-cli --test inspection --locked -- --nocapture` runs the actual compiled executable with null stdin. Original exclusively owned fixtures cover help/version, catalog parity/aliases, read-only Unicode/spaced source validation, typed exit codes, stdout/stderr separation, unsafe diagnostics, and lossless native source paths. Linux/Windows require their opaque byte/wide-name fixture; macOS APFS requires the observed EILSEQ capability refusal explicitly. Sandbox denial is not counted as APFS evidence and requires a scoped native rerun. No test writes to HOME/real agent stores, changes process-global environment, uses credentials or executes payloads. Native CI is required for the final revision. `cargo test -p agent-skills-kit-cli --test planning --locked -- --nocapture` compares both scopes/target aliases with actual library paths/attribution/write metadata and complete unchanged fixture trees. It also covers existing unchanged/conflict results, unknown targets, invalid scopes/options/missing roots, preserved parent-component refusal and ignored child-only ambient home/host overrides.

Use the already locked `serde_json` 1.0.154 directly for necessary structured output/escaping, rather than a handwritten JSON encoder. It remains maintained at [serde-rs/json](https://github.com/serde-rs/json), MIT OR Apache-2.0, declared MSRV 1.71, below the workspace's 1.85; the primary [v1.0.154 manifest](https://github.com/serde-rs/json/blob/v1.0.154/Cargo.toml) and matching cached registry manifest were reviewed 2026-10-16 UTC. The repository is non-archived with activity on that date. A fresh crates.io API recheck returned HTTP 403, so no new latest/non-yanked registry claim is made; the previously reviewed locked version is retained. It was already in the library graph, and this adds no registry package/version or runtime. No argument-parsing framework or new tool installation is introduced for this bounded grammar. Commands call library policy rather than duplicating target paths or validation.

Installation reproduction: `cargo test -p agent-skills-kit-cli --test installation --locked -- --nocapture`. Owned fixtures exercise real successful user/project installs for every core target, alias deduplication, original bytes/empty directories, fresh unchanged, valid library receipt parsing, no-approval/no-stdin refusal, dry-run/library-plan parity, preserved manual edits despite --yes, invalid/duplicate/force flags, invalid inputs and real unsafe-link refusal. Separate synthetic unit reports check committed permission failure/earlier success/not-attempted/cleanup rendering and retention of successful committed outcomes after a receipt encoding error; these are not native fault injection. The library's required ASK-012 tests supply actual permission/race/partial/interruption evidence. CLI-boundary concurrency, arbitrary output failures and real disk-full/quota failures are untested.
