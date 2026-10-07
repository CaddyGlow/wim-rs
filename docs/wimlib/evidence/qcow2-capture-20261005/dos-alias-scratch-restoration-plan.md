# DOS alias scratch restoration experiment

This proposes a new disposable scratch-tree experiment after the installation
baseline is frozen. It does not modify current aliases or waive the observed
FaceProcessor/Core FACEPR~1/~2 permutation.

[SetFileShortNameW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setfileshortnamew)
is a documented NTFS API. Current Windows requires DELETE access and
FILE_FLAG_BACKUP_SEMANTICS. Before Windows 10 1903, stronger write access and
SeRestorePrivilege were required. Empty string removes an alias; collision
returns ERROR_ALREADY_EXISTS, while case-sensitive handles or invalid names
can return ERROR_INVALID_PARAMETER. Record actual build, requested/granted
access and enabled privileges rather than assuming administrator ownership
suffices. Query volume short-name policy without changing it.

The protocol [FileShortNameInformation](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fsa/13f8d991-c37a-4156-8e29-9200c13441c9)
models the alias on Open.Link, not merely the file ID. It rejects collisions
with sibling long or short names and can reject another hardlink already having
a short name. The protocol's access/restore checks must not be substituted for
the newer Win32 contract: test the actual installed build. This means a blanket
per-inode alias setter cannot be assumed to preserve every directory entry.

Create two fresh long-name files with different payload hashes and a third
unrelated collision blocker under a new scratch parent. Add same-directory
and cross-directory hardlinks in a separate case, and one child directory.
Record all parent/name UTF16 pairs, existing aliases, link counts, file IDs,
full metadata and payload hashes before mutations. Open each intended long
entry separately with no-follow/backup semantics and DELETE access; never open
by the currently wrong short alias to select the mutation target.

First request an occupied alias on the other file and verify the documented
collision without changing either name. Then test permutation restoration by
clearing both affected scratch aliases before assigning the exact desired
aliases. Keep all unrelated names unchanged; reject a desired alias occupied
by an unrelated long name or short name. This two-phase experiment tests a
collision strategy, not a deployment transaction: interruptions between phases
can leave aliases absent and must be logged. A scratch branch can be discarded
instead of attempting recovery on an installed system.

Close and reopen after each call. Enumerate the parent with an API returning
long and short names, and open both long and intended short paths with
GetFileInformationByHandleEx(FILE_ID_INFO). Compare volume serial plus full
128-bit ID, not GetShortPathName fallback output or identical file content.
Also verify the former wrong alias no longer resolves to that file. Repeat all
hardlink/directory cases independently; inspect every affected per-entry alias,
not one representative inode. A supported clean shutdown and strict offline
NTFS directory-index/MFT comparison confirm on-disk long/short references.

Record API status, returned errors and all raw observations. Expected parent
metadata changes must be reported: the protocol updates parent times and file
change metadata. Successful scratch restoration does not establish full Setup
fidelity, safe hardlink coverage, crash recovery, or a production remediation.
The full 101,769 current alias gate and original 601 challenge remain required.
WinRE's 708 access-time differences remain a separate failed strict gate.
