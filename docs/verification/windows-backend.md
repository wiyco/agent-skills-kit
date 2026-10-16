# Windows backend implementation review

The maintainer explicitly requires successful Windows copying and native validation in ASK-012 (2026-10-16). This retains the issue's original acceptance criterion. ADR 0006's fail-closed gate remains in force while the backend is built; passing primitive tests alone must not enable or advertise the public installation path.

## Native primitives

The first implementation layer tested the real native primitives before exposing them to library operations. The second enables the read-only subset for Windows NTFS inventory; commit/cleanup primitives remain test-only until executor integration. Public Windows installation remains disabled until all layers are complete and natively verified.

- Root selection permits only fixed local drive prefixes. The drive root is opened with backup-directory/reparse-open flags; every subsequent root component is opened relative to its retained parent and rejects reparse points. Pure prefix checks reject UNC/device paths without network calls. Mapped/non-fixed drives are refused before opening the requested path. Caller-controlled ancestors remain required; Windows root ancestor aliases are explicitly unsupported.
- Descendants use a single validated UTF-16 component and `NtCreateFile` with `OBJECT_ATTRIBUTES.RootDirectory`, reparse-open behavior, an explicit file/directory type, synchronous I/O, and either open-existing or create-new disposition. There is no full-path descendant fallback, overwrite disposition or alternate stream syntax.
- Opened disk objects are checked through basic, standard, attribute-tag and 128-bit file-ID information. Every reparse tag/device, multiply-linked regular file, delete-pending object or invalid length is rejected. Native identity is not truncated to a 64-bit inode.
- Retained handles omit delete sharing, so ordinary pathname rename/deletion of those objects is denied while they are held. Read/write sharing is allowed; this does not freeze bytes, attributes or directory contents. Later inventory/verification must still compare bounded reads, digests and stamps. Kernel/user security privileges and hostile caller-root mutation remain outside the contract.
- Commit uses the held source object, a retained destination parent, a validated single component and `NtSetInformationFile(FileRenameInformation)` with replacement disabled. Its aligned variable-size SDK buffer is bounded by the component limit. There is no ordinary rename fallback, destination deletion or path-based merge.
- Cleanup uses `FileDispositionInfo` on an operation-owned handle, not a remembered pathname. This primitive does not establish ownership on behalf of its caller: the executor must supply only proven created objects and never reuse installed-payload handles as cleanup authority. Nonempty directory deletion fails.

Unsafe calls are confined to the native wrapper with live-handle, buffer-layout and ownership explanations at each boundary. The native tests establish component refusal/wide identity, exact byte I/O, create-new refusal, successful directory commit, last-moment file/empty-directory/symlink winners, reparse/hard-link rejection, parent rename denial while held and handle-based cleanup preserving an unrelated sentinel. They create only exclusive owned temporary roots and inert data. A missing Windows symlink privilege fails explicitly.

The initially evaluated `SetFileInformationByHandle(FileRenameInfo)` candidate returned Windows error 87 for an actual parent-relative directory rename in native CI (run 38913718515, job 117015834878). The implementation therefore selects documented user-mode `NtSetInformationFile(FileRenameInformation)` directly, preserving the parent handle and replacement-disabled flag. It never retries through a path-based rename. Native proof remains required before enabling the backend. This is a refinement of ADR 0006's proposed API direction, with unchanged containment/no-clobber requirements.

## Primary API evidence

Reviewed 2026-10-16; desktop Windows SDK API contracts, exercised on the exact Windows native CI revision recorded in PR QA. Older Windows versions and other architectures are not inferred from minimum API availability.

| API/document | Relevant contract |
| --- | --- |
| [NtCreateFile](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile), parameters/remarks | Root-directory-relative names, open/create dispositions, synchronous I/O, reparse-open behavior and sharing compatibility. |
| [GetFileInformationByHandleEx](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex), class table | Exact basic/standard/tag/ID structures for retained-object inspection. |
| [FILE_ID_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info), members | Volume identity and full 128-bit identifier. |
| [FILE_RENAME_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info), members | Replacement-disabled behavior, parent-relative target and byte-counted UTF-16 name. |
| [NtSetInformationFile](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntsetinformationfile), remarks/class table | User-mode Nt spelling, native handle rename class and DELETE access. |
| [FILE_RENAME_INFORMATION](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information), members | Native parent-relative name, FALSE replacement flag and exact SDK buffer layout. |
| [SetFileInformationByHandle](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfileinformationbyhandle), class table/remarks | Native handle mutation and required access rights; unsupported operations return failure. |
| [FILE_DISPOSITION_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_disposition_info), members | Handle-based deletion request; no path-prefix cleanup inference. |

## Dependency review

`windows-sys` 0.61.2 is Microsoft's generated Windows SDK binding crate, MIT OR Apache-2.0, declared MSRV 1.71. Its sole dependency `windows-link` 0.2.1 has the same license and declared MSRV. Both already existed transitively in the committed lockfile; the direct Windows-only dependency adds the WDK/Win32 features needed for these APIs. No local tools or user-level configuration are installed or changed. Typed generated layouts and declarations avoid maintaining handwritten Windows ABI structs/link signatures.

Evidence: the exact cached registry package manifests and generated modules were inspected before use; [upstream Microsoft repository](https://github.com/microsoft/windows-rs) was checked live as nonarchived with activity on 2026-10-16. Its root GitHub license is not used to override the actual crate manifests' dual-license terms. Existing Cargo.lock records both exact versions/checksums. Rust 1.85.0 compatibility and native Windows Clippy/tests still require actual CI results, not the declared dependency MSRV alone. No dependency-advisory audit is claimed by this review.

## Remaining integration gates

Directory enumeration/resource limits, local-filesystem eligibility, source snapshots, native namespace observation, overlap/collision/dedup planning, permissions/private stage ACLs, copied-byte readback, stale-plan revalidation, partial reports/receipts, concurrent installers, cleanup/interruption and final native acceptance matrix are not completed by these primitives. Inherited destination security descriptors must not silently grant staging access; source security descriptors/attributes must not be copied. Windows success support remains unvalidated until that work passes required CI and final QA.


## Read-only integration layer

Source inventory is implemented separately from public plan/apply support. It adds bounded directory enumeration through a retained-object cursor protected by an explicit per-handle scan mutex, local NTFS/remote-device checks and alternate data stream rejection. Full native stamps are retained for comparisons; ordinary portable mode intent is explicitly not ACL equivalence. See [validation](../validation.md#windows-read-only-inventory) for behavior, API considerations and tests. Windows copying remains required and gated.

Additional Microsoft primary API sections reviewed 2026-10-16: [ReOpenFile parameters/flags](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-reopenfile); [NtQueryDirectoryFile parameters/completion/scan semantics](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntquerydirectoryfile); [volume information classes](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-zwqueryvolumeinformationfile); [device characteristics layout](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/ns-wdm-_file_fs_device_information); [volume identity](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getvolumeinformationbyhandlew); [stream record layout/alignment](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_stream_info). Exact Windows CI still establishes whether these contracts work in the tested environment.

Fixed-drive selection additionally follows [GetDriveTypeW return values](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdrivetypew), reviewed 2026-10-16. A fixed-drive label alone is insufficient; retained NTFS and remote-device checks remain mandatory.


The evaluated `ReOpenFile` directory-cursor candidate failed with access denied in Windows native CI (run 38917013663/job 117026966407). Enumeration instead uses the retained directory handle directly with a per-handle mutex and explicit restart. Every Arc clone shares the same lock; there is no global state, path fallback or new root lookup. Native repeated/concurrent scan tests and finite entry budgets gate this choice. The earlier candidate failure is not counted as passing evidence.


## Namespace observation layer

Read-only destination observation retains actual root/descendant identities, missing components and owner/group/DACL preconditions, with finite SDK descriptor-size checks and matching LocalFree ownership. Source inventory and namespace observations share the same native stamp/stream validation. Windows install planning and execution remain gated.

Primary security APIs reviewed 2026-10-16: [GetSecurityInfo](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-getsecurityinfo), [GetSecurityDescriptorLength](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-getsecuritydescriptorlength), and [LocalFree](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-localfree). The fixture-only ACL mutation uses [SetSecurityInfo](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setsecurityinfo), [GetSecurityDescriptorDacl](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-getsecuritydescriptordacl), and [GetSecurityDescriptorControl](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-getsecuritydescriptorcontrol). It toggles inheritance with the original non-null DACL and restores that DACL/mode before asserting, without replacing it with an everyone-access descriptor or altering real user stores.
