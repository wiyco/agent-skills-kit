# Command-line inspection

The executable is `agent-skills-kit`; it consumes the same library catalog and validation APIs as other Rust applications. Current commands are read-only:

```text
agent-skills-kit targets list [--json]
agent-skills-kit targets show <id> [--json]
agent-skills-kit validate <source> [--json]
agent-skills-kit --help
agent-skills-kit --version
```

No arguments or `--help`/`-h` displays help. `--version`/`-V` must be used alone. `--json` is a global flag accepted once before the `--` terminator; use `--` before a positional path/ID beginning with a dash. Target IDs/aliases are case-sensitive UTF-8; filesystem source paths stay native. Relative sources are joined to the CLI working directory explicitly before calling the library; parent-traversal inputs remain subject to native library refusal. The library itself never discovers or changes CWD. Missing/extra arguments, unknown flags/targets and repeated JSON flags are usage errors. Help takes precedence when encountered; no command reads redirected stdin, prompts, or waits for a TTY.

`targets list` returns canonical definitions in library order; `targets show` accepts a canonical ID or explicit alias. Definitions show evidence/scopes/preferred writes and discovery locations. Choosing a tool is placement, not exclusive visibility: other hosts may read compatible locations. Listing a documented path is not proof of host discovery or safety on every deployment.

`validate` performs the library's bounded inert source inventory, including metadata/resource validation, unsafe entry refusal and native filesystem capability checks. It never creates destinations or executes skill content. The human result names the skill and inspected entry/byte counts. Validation is not malware review. Actual native eligibility and limitations are in [validation](validation.md) and [execution](execution.md).

Planning, approval and installation commands are dependent ASK-014 work; this initial command layer does not implement or advertise them.

## Output schema and exit codes

Without `--json`, results go to stdout and errors to stderr. Diagnostics escape terminal controls/directional formatting; untrusted error newlines/tabs are rendered as text on one line. With `--json`, exactly one compact JSON document plus newline goes to stdout, and normal diagnostic stderr is empty. No ANSI styling is emitted. A broken output pipe is an I/O failure and may prevent a complete document; the process exits 1 instead of panicking.

Successful envelope schema 1:

```json
{"schema_version":1,"command":"targets.list","status":"ok","data":{"schema_version":1,"targets":[]}}
```

`data` depends on `command`: `targets.list` is the library's catalog document; `targets.show` is one canonical target definition; `validate` contains `source`, `name`, `description`, `resource_entries`, `total_bytes`; `help` contains `usage`. `source` is the library's lossless tagged [native-path representation](receipts.md#lossless-native-paths), never a lossy display path. Nested catalog schema versions remain independent from this CLI envelope version. Data remains untrusted and observational.

Errors use `command: "error"`, `status: "error"` and `error: {"exit_code": 2, "message": "..."}` instead of `data`. Messages are human diagnostics, not stable parsable error categories. JSON syntax escapes C0, C1 and directional formatting characters while preserving their original values when decoded; machine consumers must still treat decoded strings as untrusted. No literal terminal escape/control sequence is emitted.

Exit 0 means success/help/version; 1 means validation/native/serialization/output I/O failure; 2 means invalid command/options/unknown target. No mutation or overwrite approval is implied by any exit code. Future plan/apply outcome and approval codes will be documented with those commands.

## Verification and dependencies

`cargo test -p agent-skills-kit-cli --test inspection --locked -- --nocapture` runs the actual compiled executable with null stdin. Original exclusively owned fixtures cover help/version, catalog parity/aliases, read-only Unicode/spaced source validation, typed exit codes, stdout/stderr separation, unsafe diagnostics, and lossless native source paths. Linux/Windows require their opaque byte/wide-name fixture; macOS APFS requires the observed EILSEQ capability refusal explicitly. Sandbox denial is not counted as APFS evidence and requires a scoped native rerun. No test writes to HOME/real agent stores, changes process-global environment, uses credentials or executes payloads. Native CI is required for the final revision.

Use the already locked `serde_json` 1.0.154 directly for necessary structured output/escaping, rather than a handwritten JSON encoder. It remains maintained at [serde-rs/json](https://github.com/serde-rs/json), MIT OR Apache-2.0, declared MSRV 1.71, below the workspace's 1.85; the primary [v1.0.154 manifest](https://github.com/serde-rs/json/blob/v1.0.154/Cargo.toml) and matching cached registry manifest were reviewed 2026-10-16 UTC. The repository is non-archived with activity on that date. A fresh crates.io API recheck returned HTTP 403, so no new latest/non-yanked registry claim is made; the previously reviewed locked version is retained. It was already in the library graph, and this adds no registry package/version or runtime. No argument-parsing framework or new tool installation is introduced for this bounded grammar. Commands call library policy rather than duplicating target paths or validation.
