# Windows backend implementation review

The maintainer explicitly requires successful Windows copying and native validation in ASK-012 (2026-10-16). This retains the issue's original acceptance criterion. ADR 0006's fail-closed gate remains in force while the backend is built; passing primitive tests alone must not enable or advertise the public installation path.

## Native primitives

The first implementation layer is a private module compiled only into Windows library tests. It is real native API code intended for subsequent inventory/planner/executor integration, not a simulated Windows path walker. Public Windows installation remains disabled until all layers are complete and natively verified.

- The caller-trusted absolute root is opened with backup-directory and final reparse-open flags, then inspected through the actual handle. Ancestor trust is explicit; this root operation is not a descendant walker.
- Descendants use a single validated UTF-16 component and `NtCreateFile` with `OBJECT_ATTRIBUTES.RootDirectory`, reparse-open behavior, an explicit file/directory type, synchronous I/O, and either open-existing or create-new disposition. There is no full-path descendant fallback, overwrite disposition or alternate stream syntax.
- Opened disk objects are checked through basic, standard, attribute-tag and 128-bit file-ID information. Every reparse tag/device, multiply-linked regular file, delete-pending object or invalid length is rejected. Native identity is not truncated to a 64-bit inode.
- Retained handles omit delete sharing, so ordinary pathname rename/deletion of those objects is denied while they are held. Read/write sharing is allowed; this does not freeze bytes, attributes or directory contents. Later inventory/verification must still compare bounded reads, digests and stamps. Kernel/user security privileges and hostile caller-root mutation remain outside the contract.
- Commit uses the held source object, a retained destination parent, a validated single component and `SetFileInformationByHandle(FileRenameInfo)` with replacement disabled. Its aligned variable-size SDK buffer is bounded by the component limit. There is no ordinary rename fallback, destination deletion or path-based merge.
- Cleanup uses `FileDispositionInfo` on an operation-owned handle, not a remembered pathname. This primitive does not establish ownership on behalf of its caller: the executor must supply only proven created objects and never reuse installed-payload handles as cleanup authority. Nonempty directory deletion fails.

Unsafe calls are confined to the native wrapper with live-handle, buffer-layout and ownership explanations at each boundary. The native tests establish component refusal/wide identity, exact byte I/O, create-new refusal, successful directory commit, last-moment file/empty-directory/symlink winners, reparse/hard-link rejection, parent rename denial while held and handle-based cleanup preserving an unrelated sentinel. They create only exclusive owned temporary roots and inert data. A missing Windows symlink privilege fails explicitly.

## Primary API evidence

Reviewed 2026-10-16; desktop Windows SDK API contracts, exercised on the exact Windows native CI revision recorded in PR QA. Older Windows versions and other architectures are not inferred from minimum API availability.

| API/document | Relevant contract |
| --- | --- |
| [NtCreateFile](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile), parameters/remarks | Root-directory-relative names, open/create dispositions, synchronous I/O, reparse-open behavior and sharing compatibility. |
| [GetFileInformationByHandleEx](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex), class table | Exact basic/standard/tag/ID structures for retained-object inspection. |
| [FILE_ID_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info), members | Volume identity and full 128-bit identifier. |
| [FILE_RENAME_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info), members | Replacement-disabled behavior, parent-relative target and byte-counted UTF-16 name. |
| [SetFileInformationByHandle](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfileinformationbyhandle), class table/remarks | Native handle mutation and required access rights; unsupported operations return failure. |
| [FILE_DISPOSITION_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_disposition_info), members | Handle-based deletion request; no path-prefix cleanup inference. |

## Dependency review

`windows-sys` 0.61.2 is Microsoft's generated Windows SDK binding crate, MIT OR Apache-2.0, declared MSRV 1.71. Its sole dependency `windows-link` 0.2.1 has the same license and declared MSRV. Both already existed transitively in the committed lockfile; the direct Windows-only dependency adds the WDK/Win32 features needed for these APIs. No local tools or user-level configuration are installed or changed. Typed generated layouts and declarations avoid maintaining handwritten Windows ABI structs/link signatures.

Evidence: the exact cached registry package manifests and generated modules were inspected before use; [upstream Microsoft repository](https://github.com/microsoft/windows-rs) was checked live as nonarchived with activity on 2026-10-16. Its root GitHub license is not used to override the actual crate manifests' dual-license terms. Existing Cargo.lock records both exact versions/checksums. Rust 1.85.0 compatibility and native Windows Clippy/tests still require actual CI results, not the declared dependency MSRV alone. No dependency-advisory audit is claimed by this review.

## Remaining integration gates

Directory enumeration/resource limits, local-filesystem eligibility, source snapshots, native namespace observation, overlap/collision/dedup planning, permissions/private stage ACLs, copied-byte readback, stale-plan revalidation, partial reports/receipts, concurrent installers, cleanup/interruption and final native acceptance matrix are not completed by these primitives. Inherited destination security descriptors must not silently grant staging access; source security descriptors/attributes must not be copied. Windows success support remains unvalidated until that work passes required CI and final QA.
