# Resource attribute scratch experiment preparation

Prepared only: no Windows setter has been executed. This does not establish
restoration or standard Setup preservation. No source or installed-target path
is a setter destination. ACE20 is not set.

Reproduce the source-buffer copy on the host:

```sh
python3 scripts/wimlib/prepare-resource-attribute-experiment.py \
  /data/cache/qcow2-oobe-preparation-control-20261005/offline-audit/reserved-sacl-inventory.json \
  NEW-OUTPUT.json
python3 scripts/wimlib/test_resource_attribute_experiment.py
```

The preparer validates bounded ACE18 envelopes, SID size, zero access mask,
known flags, single UINT64 relative claims, offsets and terminated UTF16 names.
It records the full immutable inventory SHA256; source UTF16 paths are provenance
only. The currently prepared input is
`/data/cache/qcow2-oobe-preparation-control-20261005/offline-audit/ace18-scratch-input.json`.
The six host tests cover accepted source bytes, rejected other ACE types,
all truncations, out-of-bounds claim offsets misaligned relative offsets, and invalid SID/mask/flags.

After actual successful OOBE, coordinate a separate disposable branch and a
new disposable NTFS volume with the installation owner. Copy the input and
`windows-resource-attribute-scratch.ps1` into that environment. For example,
substitute the actual new scratch drive for `T:`:

```powershell
.\windows-resource-attribute-scratch.ps1 -InputJson .\NEW-OUTPUT.json `
  -ScratchVolumeRoot T:\ -ReportPath .\NEW-SCRATCH-REPORT.json
```

The script refuses the system volume, creates a UUID scratch tree, and preserves
all created evidence. It explicitly enables SeSecurityPrivilege for queries,
records token privileges before/after, and requests ACCESS_SYSTEM_SECURITY on
no-follow handles. It queries raw OWNER/GROUP/DACL/SACL/LABEL/ATTRIBUTE descriptors.
It only sets ATTRIBUTE_SECURITY_INFORMATION (0x20), using copied source buffers
and a separately constructed AddResourceAttributeAce UINT64 positive control.
Each case includes an existing new child to observe propagation and a handle
without WRITE_DAC negative control. Set status, pre/post descriptors, close/reopen
ACE bytes/flags, owner/group/DACL preservation and child changes are recorded.
The negative must return ERROR_ACCESS_DENIED and preserve the descriptor.

The negative control uses the same caller with reduced handle access; it does
**not** establish the separate ordinary unelevated-token control proposed in the
feasibility assessment. An actual experiment must report this distinction.
SeSecurityPrivilege availability is required, not inferred from administrator
membership. A preparation/parser/compilation pass is not a Windows execution
pass. Scratch success alone cannot waive the installed whole-source gates.

Host PowerShell 7.6.6 parsed the complete script and compiled the embedded C#
helper successfully. No Windows native function was invoked by that check.
