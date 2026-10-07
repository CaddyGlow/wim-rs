# Windows capture: original contracts and native gates

`original.json` records 21 actual Windows guest cases through the unchanged
original header and a MinGW/MSVCRT dynamic caller. `missing-export-red.json`
records the initial native DLL lacking capture exports: exit 77 is the caller's
missing-symbol guard, not a wimlib API return code. `acl-original.json` and
`acl-native.json` now match all 21 actual guest cases, including byte-for-byte
security descriptors returned by the directory iterator. The same MinGW caller
and MSVCRT are used for both DLLs. Only corresponding live NTFS access timestamps
are normalized as described below; the raw output remains retained.

The preserved, owned fixture is `C:\wim-capture-20261003\source-é漢` in the
disposable `wim-abi-20261003` guest. It contains two directories, five file names,
three hardlink aliases of a 12-byte payload, one empty file, and a four-byte binary
payload under a Unicode name containing a surrogate pair. Readonly and hidden
attributes are explicit. The source manifest records actual creation/write times;
PowerShell's historical timestamp request does not remain effective for the
readonly hardlink inode, so this evidence uses its observed NTFS values rather
than claiming every timestamp is 2010. An initial preparation/warmup precedes
the stable baseline. Source bytes, attributes and creation/write times match
before and after the recorded matrix. NTFS access-time updates on actual reads
and enumeration are normal source filesystem behavior.

The NO_ACLS case writes a real 2,076-byte WIM. Its actual bytes are returned through QGA
and checked using the unchanged original host reader. Verify and apply both
return 0; output names, bytes and the three-alias hardlink group are correct.
The host reader explicitly reports that Linux apply ignores readonly/hidden and
DOS names. Those flags are measured in the Windows metadata inspection, not
claimed as host filesystem restoration.

`root-name-dos-red.json` retains the first native WIM's nonempty-root warning and
missing DOS names. The scanner now uses the original's NT class 21 alternate-name
query and clears the image root's primary/DOS names. `native-fixed.json` closes
those regressions. `raw-root-original.json` and `raw-root-native.json` also inspect
the written metadata bytes: both root name lengths are zero, metadata size is 920,
and SHA-1 matches the lookup descriptor. The original reader reports the same four
DOS names for both WIMs.

`default-acl-write-original.json` and `default-acl-write-native.json` independently
write 2,404-byte WIMs with default ACLs. Both pass the unchanged host reader and
an additional independently loaded original **Windows** DLL's verify/apply APIs.
The resulting seven-entry NTFS manifests match exactly in file bytes, attributes
and owner/group/DACL SDDL. Before-write C iterator SD bytes also match. This proves
native capture/writing of this default-security fixture and original Windows apply;
it does not claim a native Windows extraction backend.

`privileges-original.json` records ten actual process-token lifecycle cases,
including default and strict init flags, DONT_ACQUIRE combinations and repeated
init/cleanup. `privileges-sticky-red.json` retains the old native cleanup failure
after externally re-enabling Backup privilege and then using DONT_ACQUIRE. The
updated `privileges-native.json` matches all ten cases, including that sticky
lifecycle and a fresh DONT_ACQUIRE lifecycle which leaves externally enabled
Backup untouched. The caller runs as SYSTEM with all five tested rights assigned.
`dont-acquire-strict-original.json`/`dont-acquire-strict-native.json` additionally
match actual strict-ACL capture failure 63 when initialization leaves Security
privilege disabled. Restricted-token assignment failures remain a separate gate.

`protected-acl-original.json`/`protected-acl-native.json` use a second preserved
fixture with explicit owner/group, a protected root ACL with inheritable allows,
and a protected hardlink inode ACL containing a write denial plus SYSTEM/admin
allows. The 2,628-byte WIMs match in capture SD bytes, raw-root/digest checks and
independent host results. The original Windows DLL verifies and applies both;
their NTFS manifests match in owner/group/DACL SDDL, raw attributes and file bytes.

`multi-original.json` records five same-inode controls: ACL then NO_ACLS, reversed
flags, a DACL change inside one update call, and equivalent separate-call variants.
The initial iterator views already matched, but `multi-write-red.json` exposed
unused security descriptors in the written metadata and a retained source DOS
name after renaming the top-level captured file. `multi-write-original.json` and
`multi-write-native.json` now match all five real writes: metadata sizes
480/352/480/480/624, security table counts 1/0/1/1/2, exact table bytes and actual
per-node security IDs. SHA-1 validates against each lookup entry. Same-call
aliases remain one hardlink group; separate calls remain distinct file inodes.
The first security metadata wins within one capture session, including absence
under NO_ACLS. A new update call observes the changed DACL. The controlled fixture
is separate from the preserved directory fixtures and intentionally changes its
own ACL during these tests.

Original scan contracts:

- VERBOSE reports postorder entries with extended absolute `\\?\C:\...` paths.
  Final totals are 2 directories, 5 non-directories and 16 unique bytes. Hardlink
  aliases increment file counts but do not repeat byte totals.
- Without VERBOSE, all three scan counters remain zero, including SCAN_END.
- NO_ACLS together with STRICT_ACLS succeeds; NTFS, DEREFERENCE and UNIX_DATA
  return Unsupported (68) in this Windows build. Conflicting RPFIX flags and
  unknown flags return InvalidParam (24).
- Explicit configuration with WINCONFIG returns 24; missing configuration
  returns InvalidCaptureConfig (83). Unknown configuration sections are ignored.
- Callback abort at scan begin/dentry/end returns 76, with no committed image.
  An invalid callback status returns 77.
- Pending lookup resources have zero hashes, metadata reported size zero,
  regular resource sizes 12 and 4, and hardlink reference count 3. Actual hashes
  appear after writing.

Run `scripts/wimlib/check-windows-capture-api.py --qga-socket <owned-socket>` for
the original. Its caller is `scripts/wimlib/probe-windows-capture.c`, compiled
using the MinGW toolchain recorded in `../native-windows-abi/original-build.json`
with `-municode -Werror -I/tmp/wimlib/include`. Supply `--dll`,
`--implementation`, `--baseline original.json` and a separate `--output` for native
execution. Comparison keeps creation/write timestamps and historical access times
exact; only corresponding live access timestamps are normalized, requiring each
native value to be within 24 hours of the original. Raw output remains retained.

Default/strict and protected allow/deny ACL capture works for these fixtures.
Restricted tokens, ADS, reparse points, VSS, remote filesystems, allocator-failure
behavior, encryption, extended attributes and object IDs remain separate gates until their
actual guest evidence is added. A native Unsupported return for an unimplemented
feature is an honest partial scope, not parity with the original's successful
capture. No Windows servicing or installation result follows from these fixtures.
