# ADR 0002: Explicit plans, no-clobber copies, bounded trust

Status: accepted direction; the detailed v0.1 contract was reviewed in ASK-008 and recorded in ADR 0006.

The reviewed v0.1 contract, implementation gates, and native regression scenarios are in [the threat model](../threat-model.md) and [ADR 0006](0006-native-copy-safety.md). The maintainer accepted the contract and limits on 2026-10-16; exact implementations still require native evidence.

Plan and apply are separate. Planning can inspect the filesystem but cannot write. Applying revalidates preconditions and uses reviewed staging/no-replace behavior. Conflicting content fails by default; --yes does not bypass that policy.

The MVP rejects untrusted descendant links/reparse points and special files. Cleanup is limited to paths still owned by the current operation. Source/destination overlap is rejected. Privilege-bearing permission bits are not propagated.

A multi-target operation is not a global transaction. Reports identify successes, no-ops, failures, and unattempted targets. No guarantee is made that a copied skill is safe for a host to execute.

Consequences: ordinary `exists()` plus `rename()` is insufficient as a race-free no-clobber design. Platform capabilities and unsupported concurrent/adversarial conditions must be explicit. Destructive replacement and link installation require separate ADRs.
