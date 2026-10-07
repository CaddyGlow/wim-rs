# Reserved security metadata restoration feasibility

This is a primary-documentation feasibility assessment, not restoration evidence.
No source or failed installation was modified.

Resource attributes (ACE18) have a documented user-mode setting route.
[SECURITY_INFORMATION](https://learn.microsoft.com/en-us/windows/win32/secauthz/security-information)
identifies ATTRIBUTE_SECURITY_INFORMATION as resource properties stored in
SYSTEM_RESOURCE_ATTRIBUTE_ACE and requires WRITE_DAC to set them.
[SetSecurityInfo](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setsecurityinfo)
accepts that selector and a SACL pointer. SACL_SECURITY_INFORMATION and
SCOPE_SECURITY_INFORMATION additionally require enabled SeSecurityPrivilege.
[AddResourceAttributeAce](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-addresourceattributeace)
provides a documented construction API. The same generic setter can propagate
inheritable entries to children, so an experiment needs an isolated tree.

A concrete next experiment uses a new disposable NTFS volume, a newly created
directory and child file, and copies of the two source ACE18 buffers, never their
source paths. Open no-follow handles with explicit READ_CONTROL/WRITE_DAC;
record token privileges and owner/DACL first. Call SetSecurityInfo with
ATTRIBUTE_SECURITY_INFORMATION only, providing the validated ACL containing the
source resource attribute. Record Win32 status; close/reopen and query attributes
and raw full descriptors independently. Verify ACE18 bytes, flags and claims,
owner/group/DACL preservation and any inheritance changes. Include a plain
attribute constructed with AddResourceAttributeAce as a positive control and
a same-token handle without WRITE_DAC as a negative control. This tests handle
rights; it does not establish behavior under an unelevated account. If a
separate full SACL setter is tested, explicitly enable SeSecurityPrivilege and
request ACCESS_SYSTEM_SECURITY; do not infer that administrator membership alone
provides the requested access. A passed scratch experiment would justify a
reviewable deployment hook prototype, not restoration on failed targets or a
claim that standard Setup preserves metadata.

Process trust labels (ACE20) have no established supported arbitrary setter.
Microsoft's driver [SECURITY_INFORMATION reference](https://learn.microsoft.com/en-us/windows-hardware/drivers/ifs/security-information)
marks PROCESS_TRUST_LABEL_SECURITY_INFORMATION reserved. Existing
portable-metadata-apply-and-trust-probe.json supplies actual scratch evidence:
selector 0x80 and 0x10080 returned STATUS_ACCESS_DENIED despite the listed enabled
privileges; BACKUP_SECURITY_INFORMATION returned success but did not restore the
ACE after close/reopen. Do not invent a supported 0x80 setter, treat success of a
backup call as restoration, or assume ownership/SeRestorePrivilege defeats the
reserved policy. Ordinary ACL ownership changes are not a proven solution.

The complete apply-stage control now shows zero ACE20/18 restoration before
first target boot. Some ACE20 entries appear in failed Setup targets later,
but the mechanism is not traced. A supported package deployment or repair
experiment may determine whether Windows regenerates labels for its own signed
packages; it cannot promise arbitrary source labels, including untrusted labels
and user-profile paths. Run it only on a new disposable branch, preserve a clean
baseline, inventory all 5,725+2 paths before/after, record package identity and
version, and never waive missing labels on the strength of a subset. No primary
documentation found here guarantees full source-exact trust-label regeneration.

Full installed fidelity therefore remains unresolved. Preserve exact descriptors
in the WIM, prove ACE18 restoration independently, isolate Setup changes, and
require whole destination comparisons after successful OOBE and servicing. A
sector clone would preserve a different artifact and does not satisfy the
requested standard Setup-from-WIM objective.

The actual V12 isolated experiment has now executed. All three setter calls
returned success, but every case changed the DACL. Two resource attributes
matched exactly after reopen; the inherited source attribute disappeared.
The no-WRITE_DAC negative calls returned access denied and changed nothing.
These are strict failures, retained in
[v12-ace18-scratch-strict-result.json](v12-ace18-scratch-strict-result.json).
The next controls must separate file versus directory and inheritance behavior
without accepting DACL changes or omitting the inherited source challenge.

[SetKernelObjectSecurity](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-setkernelobjectsecurity)
explicitly advises against setting filesystem security descriptors with that API.
It is therefore not an established supported alternative for this restoration.
