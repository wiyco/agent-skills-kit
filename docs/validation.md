# Inert skill metadata validation

The library's `validation::parse_metadata` validates explicit SKILL.md bytes and a source directory name without filesystem access, environment discovery, script execution or host invocation. It is the pure first layer of ASK-007; a bounded source inventory and the safe-copy backend are separate work. A parsed skill is not evidence of trustworthy content or successful host loading.

## Contract and preservation

Read against the [Agent Skills specification](https://agentskills.io/specification), **SKILL.md format / Frontmatter / name / description / compatibility / metadata / allowed-tools**, on 2026-10-16. Rolling specification, no format revision pinned by that page. Required name/description and known optional field types/limits are checked. Name equals the supplied source directory name; names have 1–64 Unicode lowercase/caseless alphanumeric characters or single hyphens, without edge/consecutive hyphens. The detailed Unicode wording governs the general rule; OpenCode's explicitly ASCII host grammar is checked separately. This is not Unicode normalization or a promise that every host accepts every general-spec name.

As installer portability policy, reject Windows reserved device basenames even on Unix, including COM/LPT superscript-digit forms; separators, colons, traversal, controls and trailing spaces/dots are already excluded by the name grammar. Source: [Microsoft naming conventions, Naming conventions](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file), reviewed 2026-10-16. This restriction narrows the accepted specification names for safe destination components. It does not establish filesystem containment, case/normalization collision handling or no-follow safety.

Accepted input is UTF-8 with LF or CRLF, with standalone `---` opening/closing lines. A UTF-8 BOM is explicitly rejected rather than silently stripped. Preserve all accepted bytes exactly: line endings, extension fields, YAML spellings/comments, body and supporting references. `source_bytes()` is the eventual copy source; the metadata tree is for inspection, never YAML regeneration. `body_bytes()` is uninterpreted. Unknown scalar/sequence/string-keyed-map extension data is retained with positions and explicit scalar tags; tags are never instantiated. `metadata` must be string-to-string, and known scalar fields must be YAML strings rather than coerced numbers/bools/nulls. Core scalar interpretation follows [YAML 1.2.2, Core Schema / Tag Resolution](https://yaml.org/spec/1.2.2/), reviewed 2026-10-16.

For bounded processing, reject anchors/aliases (no expansion), merge keys, collection tags, complex/non-string map keys and duplicate keys at any depth. These are explicit parser-policy restrictions, not claims that the YAML specification forbids those constructs. Unknown scalar tags are inert retained data; explicit non-string tags on known string fields produce a type error. The parser does not load files referenced by any tag or field. Typed diagnostics include original line/column where available; missing fields and whole-input budgets point to the header. Error strings and metadata remain untrusted text and require escaping in a terminal renderer.

## Budgets

| Budget | Default | Hard ceiling |
| --- | --- | --- |
| Complete SKILL.md | 1 MiB | 16 MiB |
| Frontmatter | 64 KiB | 1 MiB |
| YAML scalar/collection nodes, including keys | 4096 | 65536 |
| Nested collections, including root | 16 | 64 |

Limits must be positive and cannot exceed hard ceilings. Size checks precede parser construction/tree allocation. Pull events stop on node/depth violations before recursive tree construction; recursion is capped. Input bytes bound individual scalar/scanner work and allocation. These finite bounds are not a universal CPU-time or allocator-failure guarantee. Filesystem entry/file/total/nesting budgets will be defined and tested by the inventory layer.

## Host constraints

`SkillMetadata::validate_for_target` is deliberately separate: Claude Code local reservations use the catalog's reservation query; OpenCode additionally requires its documented ASCII grammar. Host rules and sources are in [the core catalog review](catalog.md). General names and metadata must pass first. A host check does not prove effective permissions, discovery, or complete payload compatibility, and custom/unsupported host variants are not inferred as supported. The later planner must validate selected catalog scopes and call host constraints before writes.

## Dependency review

Reviewed 2026-10-16: [saphyr-parser 0.2.2 primary registry metadata](https://crates.io/api/v1/crates/saphyr-parser/0.2.2), exact published Cargo.toml, README, parser/scanner API source, and the [non-archived primary repository](https://github.com/saphyr-rs/saphyr). Published 2026-10-15, MIT OR Apache-2.0, manifest MSRV 1.85.0 (README API comments still mention older MSRVs; use the manifest and actual CI). Its pull-event API avoids writing a YAML lexer and permits explicit budgets before constructing an owned tree. No optional debug feature is enabled. Unlike a high-level alias-expanding loader, it never needs to construct an expanded document. No network/include/object constructors are part of this use.

Alternatives reviewed: serde-saphyr 2.0.0 requires Rust 1.89, above the library MSRV; serde_yaml_ng 0.10.0's last registry update was 2024-05-26; the lower-level current parser is a smaller fit for explicit bounded processing. This is a scoped dependency decision, not a claim that another parser is unsafe.

New locked transitives: arraydeque 0.5.1 (MIT/Apache-2.0, no declared MSRV, mature fixed-capacity deque; registry release 2023-02-05), thiserror/thiserror-impl 2.0.21 (MIT OR Apache-2.0, MSRV 1.77, released 2026-09-23). Existing proc-macro2/quote/syn dependencies are reused. CI must compile the complete locked graph on 1.85.0 and stable. Cargo.lock pins checksums; this review is not an advisory audit. Preserve **both** upstream author license sets for saphyr-parser: MIT notices are copied verbatim in [third-party/saphyr-parser-0.2.2](third-party/saphyr-parser-0.2.2/ChenYuheng-MIT) and [Ethiraric-MIT](third-party/saphyr-parser-0.2.2/Ethiraric-MIT). Later distribution packaging must carry both notices along with all other required dependency notices.

Tests cover exact CRLF/extension/tag/body preservation, malformed YAML/positions, missing/invalid types, duplicate/unsupported YAML, BOM/encoding/delimiters, Unicode versus host ASCII names, portable device/traversal names, all required length/budget boundaries, reserved host names and an original deterministic adversarial corpus. They invoke no embedded command. This corpus is regression coverage, not exhaustive fuzzing. Pure tests require no skill-directory writes; filesystem tests must use owned temporary roots.
