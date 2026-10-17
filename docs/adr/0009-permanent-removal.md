# ADR 0009: Explicitly approved permanent skill removal

Status: implementation contract for the maintainer-authorized ASK-032 scope (#113), recorded 2026-10-17. This is a design to be verified, not an implemented removal guarantee. Backend/API refinements must be documented and natively tested before enabling removal or publishing the first CLI alpha.

## Decision and authority

The maintainer requested permanent deletion after previewing contents and shared-reader impact and giving explicit confirmation. The earlier copy-only boundary remains for installation/update/replace; this narrowly authorized separate operation does not permit force replacement, links, remote acquisition, general recovery, or receipt-driven uninstall. Both Cargo packages remain `publish = false`.

Use a separate library removal plan/approval/apply API. The CLI only selects, renders and confirms it. An explicitly selected existing or unmanaged skill can be approved; prior installation by this program is not required or asserted. A receipt, recognized prefix, recorded path, parsed JSON, discovery row, digest alone or generic copy `--yes` grants no removal authority.

The plan must be read-only, bounded and based on a complete successful native inventory of the selected immediate skill directory under an explicit catalog/context root. Retain container namespace identity; capture the selected root identity, complete entry identities/types/permissions/lengths/digests and original metadata bytes. Unsafe/unreadable/invalid/incomplete observations cannot become approved deletion targets. No global search or deletion of a real home, host container, unknown entry, parent directory, or sibling is authorized.

Preview every intended file/directory removal including the selected root, its exact native location, total bytes, and all catalog-known native/compatibility readers of that physical location under available explicit context. Distinguish confirmed reader matches, unexpanded ancestor rules and unresolved/unknown reader evidence; never imply exclusive visibility or knowledge of all third-party tools. Record warnings and catalog identity with the preview.

An approval must refer to this exact removal plan. Terminal confirmation must name permanent deletion and the selected skill after the complete preview. Scripted confirmation must be operation-specific and tied to a previously displayed complete plan fingerprint; derive a fresh read-only plan and reject a changed fingerprint. A fingerprint is a correlation/precondition mechanism, not a secret, persistent ownership credential or substitute for native revalidation. The library's approved wrapper must not be deserializable from receipts or plan JSON. Applying consumes the approved request rather than silently replanning/reusing it.

## Native operation strategy

Before any write, recheck container binding and the complete selected inventory. Release inspection handles that prevent native deletion; reacquire the selected object relative to the retained container and compare its full native identity and current inventory. Windows sharing refusal remains a real failure; never force another process's handles closed or fall back to a path delete.

Create an exclusive operation-private same-filesystem staging enclosure with the existing private-permission policy, retained ownership identity and finite collision budget. This is an internal destructive-operation boundary, not a user trash feature or recovery promise. Do not broaden permissions or alter the selected payload's ACLs/attributes merely to make deletion succeed.

Detach only the selected immediate name into that enclosure using a reviewed native no-replace operation. On Windows, target the verified DELETE-capable held source object with the retained-parent native rename. On Unix, rename operates on a parent/name pair rather than conditioning on an inode: revalidate immediately before detach and verify the actual detached object's full identity/inventory afterward. Do not describe the Unix check/rename pair as an atomic identity-conditioned operation.

No payload unlink is allowed until the detached object is proven to be the approved object with approved contents. Expected rename-induced root metadata changes must be distinguished narrowly from payload edits; do not ignore descendant mutations or all root preconditions. If detachment captured a different or stale object, attempt native no-replace restoration to the original immediate name before any unlink. A concurrent new occupant must survive. If restoration fails or the binding cannot be established, retain the detached object privately and report its exact location and uncertainty; never erase it as cleanup or overwrite the new occupant.

After successful detachment/revalidation, remove only the approved entries, deepest first, using retained no-follow parents and current identity/type/content checks. On Unix use anchored `unlinkat`/empty-directory removal inside the private enclosure. On Windows use verified DELETE-capable object handles and file disposition; close relevant handles before claiming completed removal. Never recursively remove by pathname, follow links/reparse points, clear read-only/ACL restrictions, or automatically include new/unrecognized contents. A directory must be empty when removed. Bound all opens, enumeration, revalidation and reporting.

Private-stage cleanup is limited to proven operation-created objects. A stage-looking pathname or the pre-existing selected payload is not general cleanup authority. Lost identity or namespace control stops cleanup and returns a diagnostic. No automatic deletion of orphan stages on later runs is authorized.

## Supported concurrency and failure boundaries

The caller must control the selected context/container and ancestor namespace, and stop processes writing the selected skill before approval/apply. The CLI must display this obligation with the permanent/no-undo warning. The existing privileged attacker, hostile same-identity staging mutation/reparenting, malicious filesystem, changing mount topology and power-loss exclusions remain. Existing open file descriptors/mappings can retain or alter data across namespace operations; neither Unix unlink nor Windows disposition is secure erasure or an arbitrary mutable-tree snapshot.

Observable edits/replacements before approval/apply or before unlink invalidate the request rather than authorizing new contents. Deterministic tests must cover the relevant boundaries, including a Unix last-moment source-name substitution and a Windows sharing/reparse substitution. Native identity targeting/private enclosure provides containment; metadata checks alone do not promise protection against arbitrary concurrent writers between the last check and unlink. Unsupported conditions must be explicit, not hidden behind a successful result.

A successful operation permanently removes the selected namespace payload and its approved descendants. It does not clear backups, snapshots, host caches or already open handles and offers no undo/recovery guarantee. No global transaction or rollback is promised. Once the first approved entry is removed, do not restore a partial tree as if it were the original complete skill.

Failures must distinguish no-write preflight failure, detached-but-restored, detached-and-retained, partial permanent removal and full payload removal with stage-cleanup failure. Report each intended entry as confirmed removed, failed/remaining when established, or not attempted/unknown; retain native errors, original and staging paths, restoration/cleanup findings and any unrecognized entries. Do not label an unattempted entry as verified present or lose earlier removals. A process kill can leave a private full or partial payload without a report. Later inspection is manual; a stage prefix/receipt never authorizes automatic continuation.

## Required evidence before enabling removal

- Read-only bounded plan and complete contents/shared-reader preview; invalid/unsafe/incomplete inputs fail without writes.
- Exact operation-specific confirmation; cancellation, generic `--yes`, wrong/stale fingerprint, EOF/partial confirmation and forged receipts cannot delete.
- Actual successful permanent removal on native Linux, macOS and Windows, with unrelated sibling/outside sentinels unchanged.
- Stale root/content/mode/binding changes, actual links/reparse points, hard links and special entries rejected without deleting their targets.
- Deterministic source substitution, detached content mutation, concurrent new original-name occupant and private-stage identity substitution; accurate restoration/retention and no clobber.
- Actual native permission/sharing/read-only denial plus deterministic mid-removal failure; complete removed/remaining/not-attempted reports and no permission broadening.
- Native process interruption before detach, after detach and during removal; only documented original/staged/partial state, no prefix-based recovery or false success.
- Full relevant formatting, compilation, Clippy, documentation, automated tests, MSRV and final-revision required CI; a separate final-SHA QA comment. Self-review is not independent approval.

## Primary references reviewed 2026-10-17

These sources describe primitives and limits, not proof that this design is implemented or correct on every host. Reuse reviewed existing FFI where applicable; any changed access/sharing/unsafe boundary needs exact-code review and native tests.

| Source | Reviewed section and implication |
| --- | --- |
| [Linux unlink manual](https://man7.org/linux/man-pages/man2/unlink.2.html) | `unlinkat` DESCRIPTION/ERRORS: directory-relative name removal, empty-directory flag and open-file lifetime. It does not condition unlink on a previously observed inode. |
| [Apple-published unlink manpage](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/man/man2/unlink.2) | DESCRIPTION/ERRORS at the fixed XNU revision: name removal and held-open lifetime; native macOS deployment behavior still requires testing. |
| [Linux rename manual](https://man7.org/linux/man-pages/man2/rename.2.html) | Existing reviewed `RENAME_NOREPLACE` contract: never replace a concurrent original-name occupant during restoration. |
| [Apple-published rename manpage](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/man/man2/rename.2) | Existing reviewed `renameatx_np(RENAME_EXCL)` path-pair primitive; no atomic source-inode condition is implied. |
| [Microsoft CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew) | `dwShareMode` and attributes: open-handle share restrictions persist until close; delete access also covers rename; read-only data cannot simply be deleted. The backend still uses reviewed retained-parent native opens, not full-path ancestor traversal. |
| [Microsoft SetFileInformationByHandle](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfileinformationbyhandle) and [FILE_DISPOSITION_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_disposition_info) | Handle access/class/structure and `DeleteFile`: held-object deletion intent. Correct handle lifetime, child sharing and completion are additional native test obligations. |

No new library API is implemented by this ADR. Exact public names/representation are finalized with the implementation; all guarantees above remain subject to its native evidence gate.
