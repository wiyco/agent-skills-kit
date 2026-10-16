# ADR 0006: Anchored access and native no-replace commits

Status: proposed for explicit maintainer review in ASK-008; required before ASK-010.

Adopt the [local-copy threat model](../threat-model.md) as the v0.1 safety contract. Retain the architecture's plan/apply separation and copy-only policy. Root canonicalization is a caller-authorized trust boundary; every descendant remains subject to anchored no-follow validation.

Use native exclusive same-filesystem directory commit primitives: Linux `renameat2(RENAME_NOREPLACE)` and macOS `renameatx_np(RENAME_EXCL)`, after verified anchored traversal. For Windows, evaluate a retained-handle strategy and replacement-disabled `SetFileInformationByHandle`; execution stays unsupported until the ancestor/reparse/sharing behavior is reviewed and tested. Missing capability on any OS is an explicit failure, never a fallback to `exists()` plus ordinary rename. No platform is currently advertised as installation-tested.

Caller-controlled roots/ancestors and operation-private staging are prerequisites. The contract does not promise a snapshot of an arbitrarily mutating filesystem or protection against privileged attackers, hostile same-identity mutation of staging/reparenting, malicious filesystems, or changing mount topology. Document and reject known unsupported cases. Ordinary competing installs and source/destination edits are covered by precondition checks, byte verification, anchored operations, and the commit primitive, with native deterministic regression tests required before enabling support.

Reject links, reparse points, multiply linked regular inputs, and special entries by default. Bound resource use, strip privilege-bearing metadata, and never execute payload instructions. Cleanup requires retained operation ownership/identity; a prefix or path is insufficient. Return typed failures and complete per-target outcomes; earlier successful installs survive later target failures. Atomic visibility within one supported filesystem is not a global transaction or a promise of crash durability.

The source evidence and TM-01 through TM-22 scenarios are recorded in the threat model. ASK-007, ASK-009, ASK-010 and ASK-012 implement and verify their relevant layers. No dependency or unsafe binding is approved by this ADR alone; exact backend choices require review against current API documentation, maintenance/license/MSRV evidence, and native behavior. This ADR is a contract decision, not evidence that those tests already passed.
