# ObjectID extended-info scratch experiment preparation

Prepared only: no Windows FSCTL was executed, no source or installed-target
metadata changed. This experiment cannot establish a whole-system repair or
standard Setup fidelity. Existing ACE18 preparation remains separate.

Primary references establish the boundary:

- [FSCTL_SET_OBJECT_ID_EXTENDED](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_set_object_id_extended)
  modifies extended user data, preserving the file's actual ObjectID.
- [MS-FSCC request layout](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fscc/757bc8fa-f24f-44cf-86dc-adb2c113589d)
  specifies a 48-byte opaque input, distinct from the volume-unique first16.
- [FSCTL_CREATE_OR_GET_OBJECT_ID](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_create_or_get_object_id)
  creates an identifier if absent; it is deliberate mutation during scratch setup.
- [FSCTL_GET_OBJECT_ID](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_get_object_id)
  is the subsequent observation operation.

SDK constants independently match GET=0x9009c, CREATE_OR_GET=0x900c0,
SET_EXTENDED=0x900bc. No SET_OBJECT_ID, deletion, or supplied source first16
appears in the experiment. The preparer copies only bytes16..63, rejects invalid
sizes and all-zero birth32 challenges, deduplicates equivalent alias challenges,
and records the immutable fixture hash and provenance paths.

Reproduce preparation on the host:

```sh
python3 scripts/wimlib/prepare-object-id-scratch.py \
 /data/cache/qcow2-oobe-preparation-control-20261005/offline-audit/fixture-metadata.json \
 NEW-OBJECTID-INPUT.json
python3 scripts/wimlib/test_object_id_scratch.py
```

Current prepared input:
`/data/cache/qcow2-oobe-preparation-control-20261005/offline-audit/object-id-scratch-input.json`.
Four adversarial host tests validate exact extended48 extraction, all short/wrong
lengths, zero identifier/birth challenges and invalid hex.

Coordinate execution with the installation owner only after an immutable
installed baseline has been audited. Use a separate disposable branch, attach
a new disposable NTFS scratch volume, and substitute its actual drive letter:

```powershell
.\windows-object-id-scratch.ps1 -InputJson .\NEW-OBJECTID-INPUT.json `
 -ScratchVolumeRoot T:\ -ReportPath .\NEW-OBJECTID-REPORT.json
```

The script rejects the system volume, creates a new UUID tree, opens each new
scratch file with CREATE_NEW and no-follow semantics, and invokes CREATE_OR_GET
once. It requires nonzero, distinct scratch-generated first16 identifiers. It
records full64 before, supplies copied extended48 to SET_EXTENDED, flushes and
closes, then reopens through long path, new hardlink and returned short path.
All observations use GET only. Each must retain first16, match all source48 and
return the same complete 24-byte FILE_ID_INFO. A returned short path equal to the
long path is explicitly recorded as no distinct DOS alias, not an alias pass.
No source provenance path is opened. Kernel/token/input hash/status/raw values
are recorded. Created evidence is retained; there is no cleanup step.

After execution, preserve the report and request supported orderly shutdown.
Freeze the scratch disk and backing identities without reopening it in Windows.
Use strict native QCOW2/partition/NTFS readers to reject dirty/encrypted volumes,
then inventory the scratch tree with `frozen_metadata` and independently verify
ObjectIdRaw for every long/hardlink entry equals the online full64, including the
new first16 and source extended48. Validate native short/long file identities
where a distinct DOS alias exists. Hash both reports and retain paths as UTF16;
any missing path, partial identifier or byte change is a failed gate.
`independent_offline_verified` remains false in the online report and must only
be supported by a separate actual offline report. Scratch success does not
identify or prevent the post-first-boot operation that zeroed V12 extended48.
