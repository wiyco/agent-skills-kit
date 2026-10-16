# Local-copy threat model

Status: proposed v0.1 contract for ASK-008; maintainer review is required before the executor in ASK-010 begins. This document specifies requirements and regression scenarios, not implemented or tested installation guarantees.

## Assets and trust boundaries

Protect existing source/destination content, filesystem containment, plan identity, and accurate reports. Skill names, SKILL.md, resource filenames/content, catalog extensions, stored reports, and fetched evidence are untrusted data. Instructions in a skill never control the installer or implementation agent. Do not execute scripts, hooks, Git commands, model calls, or shell interpolation derived from them.

The caller supplies explicit home/project/workspace context, scope, targets, and a local source. The library does not mutate process environment, prompt, exit, acquire remote content, or infer authorization from directory existence. Host environment overrides are path inputs, not commands. Selecting a target grants placement at its reviewed destination, not exclusive host visibility or trust in the payload.

Root selection is an explicit trust decision. The caller must control the approved root and its ancestor namespace against hostile replacement, mount changes, and privilege changes for the operation. Canonicalization may resolve the selected root's aliases at that boundary; record its identity and resolved location. It is not permission to follow descendant symlinks. Source and destination roots are opened/validated independently and retained through execution where supported. A string prefix is not a containment check.

Descendant entries and existing destination content remain untrusted even inside an approved root. Walk components using reviewed no-follow operations anchored to validated directories; inspect the actual opened object, not a previous path lookup. Do not change the process working directory. Reject traversal, absolute/prefixed resource paths, separators in a skill name, invalid native components, and target-specific reserved names. Preserve non-UTF-8 identity where supported; reject unrepresentable cases explicitly instead of silently converting names.

## Supported concurrency and limits

The contract includes ordinary competing installs, destination creation between plan and apply, source edits, entry replacement, I/O errors, and interruptions. These must lead to a safe documented result, not overwrite or escape. Descriptor/handle anchoring and exclusive commit enforce boundaries; metadata checks alone do not close races.

This is not a sandbox against a privileged attacker, malicious kernel/filesystem, or a same-identity process that can mutate private staging or reparent already-open directories outside the caller-controlled hierarchy. Snapshot-consistent reads of arbitrarily mutating trees, network/distributed filesystem semantics, crash durability across power loss, and mount/junction topology mutation are not guaranteed. Reject known unsupported roots/filesystems/conditions; never advertise adversarial containment that the backend cannot enforce. Caller obligations must be in the public API and CLI documentation, not hidden assumptions.

## Plan and apply contract

1. **Validate and inventory.** Read only regular files/directories. Reject descendant symlinks, junctions/reparse points, sockets, devices, FIFOs, and multiply linked regular files where identity/link counts can be verified. Do not interpret executable resources. Use explicit finite limits for entry count, file/total bytes, nesting, and manifest parsing; ASK-007 defines and tests their values. Avoid blocking on a substituted FIFO and never read data from a handle whose regular-file type is unverified.
2. **Plan without writes.** No locks, receipts, staging, directory creation, or hidden state. Ordinary kernel read-access bookkeeping is not an installer write. Record source entry identities, relative paths, types, content digests, lengths, ordinary executable intent, and destination/root preconditions. Stable ordering is required. Reject source equals/contains destination, destination contains source, and overlapping destinations using resolved roots, component boundaries, existing identities, and native path semantics. Detect case/normalization collisions for the destination filesystem; do not use lossy strings as identity.
3. **Classify existing content.** Absent is a planned create. Verified identical regular content/structure and relevant permissions may be unchanged. Different, unsafe, unreadable, or uncomparable content is a conflict or typed failure, not an overwrite opportunity. Attribution survives deduplication of genuinely identical destinations. `--yes` approves only the described plan and is never force permission.
4. **Revalidate before writes.** Reopen/compare trusted roots and every relevant source/destination precondition. Replaced identities, changed names/types/bytes, altered executable intent, and newly unsafe entries invalidate the plan. New destination entries are not silently adopted. A digest is an integrity check, not a malware review. Preserve structured distinctions between invalid input, stale plan, conflict, permission/I/O failure, and unsupported backend.
5. **Stage per destination.** Create an operation-owned directory exclusively on the destination filesystem under a validated parent. Use private initial permissions, retain identity/ownership handles, copy using no-follow reads and exclusive file creation, and verify staged bytes against the approved inventory. Never preserve source hard-link relationships, ownership, ACLs, capabilities, extended attributes, or privilege bits. On Unix, ordinary files become 0644 or 0755 according to reviewed executable intent, directories 0755, with private staging until validation finishes. Windows uses reviewed destination inheritance without copying source security descriptors; unsupported permission semantics must be reported. Existing directories are not chmodded to make copying succeed.
6. **Commit without replacement.** Revalidate the source/stage and relevant parent preconditions immediately before a native same-filesystem no-replace commit. The destination can appear after the last observation; the commit itself must refuse replacement. Never fall back to ordinary rename, delete-then-rename, recursive merge, or copy-into-existing. A concurrent winner is a conflict/stale-plan outcome for this request, not permission to erase it. Missing native capability fails closed.
7. **Report and clean up.** Track every requested destination and target attribution, including created, unchanged, failed, and unattempted results. Retain earlier successes when another destination fails. No global transaction or rollback across targets is promised. Return receipts to the caller; persistent storage is explicit, versioned, and outside the payload.

Creating missing destination parents is an executor operation, after plan approval. Anchor and revalidate each component; record any operation-created parent separately. Concurrent pre-existing parents are untrusted and must pass the same checks. On failure, leave unrelated or nonempty directories intact; do not recursively remove a caller's existing parent tree.

## Cleanup and interruption

Remove only staging objects whose retained identity and ownership still match this operation, using anchored no-follow traversal. A recognizable prefix or remembered pathname is insufficient. If another process replaces the staging path or cleanup cannot establish identity, leave it and return an explicit cleanup diagnostic. Never recursively delete a successful destination to imitate rollback.

A process killed before commit may leave private staging. A process killed after commit may leave a complete destination without a persisted receipt. A later process must not infer ownership of either from names alone. Recovery inspects and reports; it does not automatically delete unknown data. Receipt loss does not authorize reinstall, update, or uninstall. Commit atomicity is distinct from durable persistence across power failure; fsync/flush guarantees require separate evidence before advertising them.

## Platform implementation gates

Use small platform-specific backend modules. A Rust wrapper or library is not proof of its underlying no-follow/no-replace behavior. Review exact versions, license, maintenance, MSRV, unsafe boundaries, and native error mapping before adding dependencies. No dependency is selected or added by this document.

| Platform | Selected implementation direction | Fail-closed condition and required proof |
| --- | --- | --- |
| Linux | Directory-descriptor-relative traversal; `openat2` with beneath/no-symlink constraints where available, or a separately reviewed component walker. Commit through `renameat2` with `RENAME_NOREPLACE`. | Missing syscall/filesystem support or an unproved fallback rejects execution. Verify no-follow handling of final and intermediate components, regular-file type, held identities, and exclusive directory commit on native CI. |
| macOS | Directory-descriptor-relative no-follow traversal and `renameatx_np` with `RENAME_EXCL`; use additional beneath/no-follow flags only after deployment-version review. | A published flag does not prove availability on every deployed OS/filesystem. Missing exclusive-rename capability rejects execution. Validate the installed SDK/API and actual filesystem behavior, including existing empty directories and symlinks. |
| Windows | Retained directory/file handles, explicit reparse-tag rejection, reviewed native-name handling, and `SetFileInformationByHandle` with replacement disabled and a validated destination-parent handle. | `CreateFileW` on a full path with only a final-component reparse flag is insufficient proof of ancestor safety. Until a component/handle strategy, sharing semantics, rename behavior, and cleanup are reviewed and tested natively, execution must return unsupported; do not substitute path-based move or claim Windows installation support. |

At this stage no executor exists, so **none of these platforms has validated installation behavior**. Later PRs may enable only combinations that satisfy the contract and native regression gates. Testing Windows-looking strings on Unix is not native Windows proof. Unsupported-case rejection remains testable on every OS and must be reported separately from successful installation.

## Attack and failure scenarios for ASK-012

Use original inert fixtures and explicitly created temporary roots. Deterministic barriers/hooks must target the relevant operation rather than relying on timing-only sleeps. Snapshot content, identities, directory entries, relevant permissions, and outside-root sentinels before/after; account separately for operation-owned staging and incidental kernel access timestamps.

| ID | Scenario | Required observable result |
| --- | --- | --- |
| TM-01 | Traversal, absolute paths, Windows prefixes/streams, separators and reserved names | Typed rejection; no escape or destination writes. |
| TM-02 | Root alias accepted explicitly; descendant symlink or dangling symlink | Root decision is recorded; descendant rejected without reading its target. |
| TM-03 | Junction, mount-point or other Windows reparse tag; replacement between lookup/open | Reject all unreviewed tags; no outside sentinel changes; unsupported backend remains explicit. |
| TM-04 | FIFO, socket, device, symlink loop, deep tree or excessive input | Bounded rejection without hanging or executing content. |
| TM-05 | Multiply linked regular source; hard-link alias to existing content | Reject unreviewed aliasing; no writes through the alias and no hard links created in output. |
| TM-06 | Source equals/contains destination, reverse overlap, overlapping target roots | Reject before any staging/parent creation. |
| TM-07 | Space, Unicode, non-UTF-8 components, case/normalization collision | Preserve supported identity; deterministic rejection for unsafe/unsupported cases, no lossy collision. |
| TM-08 | Plan a normal absent destination and compare complete trees | Read-only plan; no lock/temp/receipt or hidden state. |
| TM-09 | Existing identical, different, unsafe, unreadable or unmanaged destination | Validated unchanged or conflict/failure; never overwrite. |
| TM-10 | Source content/type/size/identity/execute-bit changes after planning or during copy | Reject stale data or copied bytes that do not match inventory; leave existing content intact. |
| TM-11 | Destination file, empty directory or symlink appears immediately before commit | Native no-replace commit fails; concurrent entry survives byte-for-byte. |
| TM-12 | Two approved installers race for one destination | At most one create succeeds; loser cannot replace winner or delete its staging/install. |
| TM-13 | Parent component replaced by a link between lookup/open/commit | Anchored operation stays within the supported contract or rejects; no traversal through the replacement. |
| TM-14 | Inject read, create, copy, permission, quota or commit failure | Typed failure and accurate per-destination outcome; existing content preserved. |
| TM-15 | Replace staging pathname before cleanup; deny cleanup access | No unrelated deletion; cleanup failure/orphan diagnostic is retained. |
| TM-16 | Interrupt before and after commit | Only owned staging or a complete destination may remain; later recovery does not delete by prefix. |
| TM-17 | Privilege bits, executable intent, ACLs, attributes and ownership | Only documented ordinary output permissions; source privilege/security metadata is not copied. |
| TM-18 | One destination succeeds, later destination fails, another is unattempted | Complete outcome list and attribution; earlier success is not removed. |
| TM-19 | Duplicate exact destinations and shared-reader effects | One physical operation with all attribution; no silent native-to-shared placement rewrite. |
| TM-20 | Unsupported syscall/filesystem/platform; no-replace primitive forced to fail | Fail closed; prove ordinary rename/copy fallback is not invoked. |
| TM-21 | SKILL.md or resource contains executable-looking instructions | Data is read/copied only; a sentinel script is never executed and no network/model calls occur. |
| TM-22 | Report/receipt contains non-UTF-8 paths or terminal control bytes | Preserve identity in machine output and escape terminal rendering; do not execute or silently normalize it. |

## Reviewed primary references

Reviewed 2026-10-16. These are API documentation requirements and format evidence, not runtime tests or host path evidence.

| Source | Section / applicable scope / remaining limit |
| --- | --- |
| [Linux rename manual](https://man7.org/linux/man-pages/man2/rename.2.html) | `renameat2`, `RENAME_NOREPLACE`, filesystem support and errors; proves why ordinary rename is an overwrite primitive, not that a particular filesystem passed testing. |
| [Linux openat2 manual](https://man7.org/linux/man-pages/man2/openat2.2.html) | Resolution constraints, final-symlink semantics and errors; actual kernel/flag availability must be checked. |
| [Apple-published rename manpage](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/man/man2/rename.2) | `renameatx_np`, exclusive/beneath/no-follow flags and errors; also reviewed the locally installed SDK manpage. A manpage in Apple's source distribution is documentation, not evidence that every OS version implements every listed flag. |
| [Microsoft FILE_RENAME_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info) | Replacement flag and destination-parent handle; the structure alone does not establish safe ancestor traversal. |
| [Microsoft CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew) | Reparse-point and sharing semantics; reject relying only on a final-component flag. |
| [Agent Skills specification](https://agentskills.io/specification) | Skill format, names, metadata and resources; valid format is not execution trust. |
