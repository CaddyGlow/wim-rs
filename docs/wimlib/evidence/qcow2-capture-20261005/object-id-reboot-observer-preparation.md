# Existing ObjectID scratch reboot observer

Prepared GET-only observer; reboot execution is owned by the VM owner and must
be reported separately. The stopped scratch parent remains immutable:
`diagnostic-scratch.qcow2`, SHA256
`e18af50347659268f1edc0a037b6c9b8b0614e9fd778bd0cdc6c8f9e7e27940b`.
Attach a **new writable QCOW child** backed by that image to the separate OS
experiment child. Never attach the frozen image writable or boot the immutable
installed baseline. Preserve the original online/offline scratch evidence.

Input `/data/cache/qcow2-v12-offline-audit/object-id-reboot-observer-input.json`
SHA256 `cbbca628adfac879a743fece7fd9768951851a99f9f502b91e024098ba381af8`
binds the exact original raw UTF16 paths, full64 ObjectIDs and complete24B
FILE_ID_INFO. It includes three original observations; two distinct long/hardlink
paths and a duplicate fallback path, **no distinct DOS alias**. Both preparer and
guest hardbind the existing `T:\objectid-scratch-8e13fcf95dd746739b3ba4f1b14a64af`
tree and reject other roots, dot/dotdot, forward slashes, empty components and NUL.
Selectors are never rewritten to hide a mount or identity change.

Reproduce preparation using a new output path:

```sh
python3 scripts/wimlib/check-object-id-reboot-observer.py prepare \
 /data/cache/qcow2-installation-autologon-disabled-1005-v12/diagnostic-scratch-object-id-report.json \
 NEW-OBSERVER-INPUT.json
python3 scripts/wimlib/test_object_id_reboot_observer.py
```

Run the latest script on the disposable child before and after a supported
normal reboot, preserving distinct report names:

```powershell
.\windows-object-id-reboot-observer.ps1 -InputJson .\NEW-OBSERVER-INPUT.json `
 -ReportPath .\NEW-OBSERVER-REPORT.json
```

The native helper uses OPEN_EXISTING with no-follow handles, GET_OBJECT_ID
(0x9009c) and FileIdInfo queries only. It contains no ObjectID creator, setter,
short-name setter or repair. It records last boot/observation UTC and exact input
hash, closes handles, retains missing/changed values as failures, and never
creates a replacement challenge. Report files are the deliberate output writes
on the disposable OS branch. Independently validate each report:

```sh
python3 scripts/wimlib/check-object-id-reboot-observer.py validate \
 NEW-OBSERVER-INPUT.json NEW-OBSERVER-REPORT.json
```

Seven meaningful host tests pass: full64 equality, extended48 loss with unchanged
first16, file-identity replacement, absent ObjectID, partial16 result, changed raw
selector, and subtree/drive escape. Host PowerShell parsing and C# compilation
pass; these checks invoke no Windows native functions. Actual attachment,
pre/post reboot observations and final clean offline persistence must be proven
with separate VM-owner and native-reader evidence. A scratch pass would not
establish installed-image restoration or identify the original firstboot loss.
