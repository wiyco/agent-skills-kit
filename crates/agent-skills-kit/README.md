# Agent Skills Kit (Rust): library

**WIP — not ready for use.** This is an unpublished development candidate, not a
supported release. Do not use real skill stores. No crate installation command,
released version or public binary download is available from this project.

The synchronous library owns the bundled evidence catalog, explicit-context
resolution, inert skill validation, read-only planning, guarded native copy and
versioned receipts. The repository CLI consumes this API; other Rust applications
do not need the CLI. There is no runtime network acquisition, model invocation,
terminal prompt, process exit or implicit global environment mutation.

```rust
use agent_skills_kit::catalog::Catalog;

let catalog = Catalog::bundled()?;
let codex = catalog.get("codex-cli").expect("bundled alias");
assert_eq!(codex.id, "codex");
# Ok::<(), agent_skills_kit::catalog::CatalogError>(())
```

For write operations, supply explicit owned temporary home/project roots, inspect
the library plan and apply only after approval. Different existing content is
never force-overwritten; several destinations are not a global transaction.
Copying is not sandboxing or a trust review of the installed skill. Never execute
untrusted skill content during tests.

The declared MSRV is Rust **1.85.0**, Edition 2024; the repository development pin
is **1.99.1**. There are currently no optional public features. Native backend,
filesystem and real-host limitations remain separate from lexical path support.
Read the exact candidate CI/QA evidence before inferring support for an OS, host,
version, architecture or filesystem.

- [Public API and external consumer](https://github.com/wiyco/agent-skills-kit/blob/main/docs/api.md)
- [Architecture and safety contract](https://github.com/wiyco/agent-skills-kit/blob/main/docs/architecture.md)
- [Execution guarantees and limits](https://github.com/wiyco/agent-skills-kit/blob/main/docs/execution.md)
- [Target evidence matrix](https://github.com/wiyco/agent-skills-kit/blob/main/docs/targets.md)
- [Actual host smoke results](https://github.com/wiyco/agent-skills-kit/blob/main/docs/verification/host-discovery.md)
- [Publication gates](https://github.com/wiyco/agent-skills-kit/blob/main/docs/release.md)

Original project material is licensed **MIT OR Apache-2.0**; both full texts are
included. Third-party material retains its own terms. The package includes the
two saphyr-parser 0.2.2 MIT notices under `third-party/`; dependencies retain their
own licenses and notices. A project license is not approval to relicense them.
