# Recovered capture release gate audit

This audit follows `docs/qcow2-to-wim-plan.md`. It does not claim installation
completion. Reports of earlier success remain historical when cleanup removed
their original files; reconstructed artifacts need their own identities.

| Requirement | Evidence and remaining gate |
| --- | --- |
| Immutable generalized source | `prepared-source-recovery.json`, `source-generalization-recovered.json`, `recovered-source-preservation.json`: recovered child matches the frozen SHA-256, and all authorized parents survive. Never boot this source. |
| Backing/compressed/zero mappings and partition validation | Parser regression and independent QEMU/GPT reports establish tested supported profiles. Unsupported encryption, external data files, extended mappings, active snapshot selection and extended MBR remain explicit rejections, not full format support. |
| Bounded traversal, cancellation and publication | `parser-controls-gates.json`, `capture-regression-gates.json`, and `sixth-parser-budget-diagnosis.json`; full recovered capture passes unchanged budgets in `capture-report-recovered-check.json`. These are parser/application gates, not runtime metadata evidence. |
| Native whole-Windows capture | `capture-report.recovered-input.json` and `capture-report-recovered-check.json`: 176,484 nodes, omission audit and unchanged input hashes. The producer is bound by `recovered-producer.json`. |
| Independent WIM and recovery inspection | `independent-oracle.recovered-input.json`: 18 gates, all 331 fixture streams, full WIM verification, recovery verification, source OOBE task bytes, identity properties, and unbound recovery schema. Linux extraction does not restore Windows metadata. |
| Whole-source Microsoft comparison | `regenerated-independent-microsoft-whole-comparison.json` compares current native WIM 749c…8ea8 against recovered full Microsoft WIM 9ba5…5b09: all 176,484 paths, 34,007 hardlink groups and Windows XML fields agree, with documented metadata transformations and zero unexplained security differences. `regenerated-wim-reserved-sacl-source-equivalence.json` independently proves source-exact full raw descriptors and ACEs for all 5,725 ACE20 paths and both ACE18 paths in the regenerated WIM. This completed archive gate does not waive installed metadata failures. |
| Portable Windows metadata | `portable-metadata-policy-gates.json` and `portable-metadata-apply-and-trust-probe.json` record their historical independent apply fixtures. The tiny full64 object-ID claim currently lacks retained raw source/target observations after cleanup; tiny-object-id-evidence-audit.json does not independently substantiate its nonzero extended48 challenge. Current archive raw64 preservation is separately proven; destination apply/Setup preservation remains required. |
| Setup media and ISO | `media-report.recovered-input.json` records published ISO b0bd…298a, one install image, Setup boot index 2, verified WinRE and all 46 absolute links, with no warnings. `independent-iso-boot.recovered-input.json` passes catalog checksum and exact BIOS/EFI boot-asset byte comparisons. That independent check covers El Torito/catalog assets, not full UDF payloads or firmware execution. V6/V7 standard Setup used the published media; their OOBE failures remain separate below. |
| Fresh Windows Setup/OOBE and identity | Failed on recovered V6 and V7: standard Setup reaches IMAGE_STATE_COMPLETE with zero Setup flags, but readiness is false and no Explorer desktop exists. V7 creates capturegate, then msoobe creates defaultuser0 and changes the autologon username; successful readiness remains required. Setup flags alone do not establish OOBE success. |
| Complete destination metadata | Failed V6 diagnostic: all 331 streams and listed fixture metadata match, but payload/alias extended 48 object-ID birth bytes are zero. Of 5,727 reserved-SACL paths, 4,788 are exact, 723 differ only in inherited bits, and 216 lose ACEs; both resource-attribute ACE18 paths lose their ACEs. Native WIM full object IDs are source-exact. See v6-offline-diagnostic-metadata.json. No waiver or restoration has converted this into a passing gate. |
| Servicing and reboot | Pending: actual destination health scan, successful SFC, representative servicing and observed normal reboot. Host tests cannot substitute. |
| WinRE registration, equivalence and boot | Pending: destination-bound registration and actual WinRE UI boot. If Setup changes the recovery container, independently verify the complete source/target logical manifest and streams; a request to boot recovery is insufficient. |
| Current host and Windows checks | Timestamped recovered checks pass in `host-validation-recovered-current.json`: 2,367 tests passed, 51 ignored, full denied-warnings Clippy, format and Windows MSVC check. Concurrent later archive/WIM allocator refactors are not covered; post-validation-source-fingerprint.json records a separate observation, not validated-byte provenance. Later production edits need appropriate revalidation. |
| Optional ESD and BIOS | No release claim. Each requires separate independent compatibility and installation evidence if added to scope. |

Completion requires every applicable pending gate above. No amount of passing
archive inspection or host tests establishes a successful Windows installation.

## Source autologon preparation investigation

`source-oobe-registry-sanitized.json` proves the frozen source contains only
standard built-in accounts and deploy RID1000, with no capturegate/defaultuser0.
Autologon configuration is absent; plaintext DefaultPassword is present but empty.
An LSA DefaultPassword secret key exists, but its contents were never read.
These facts do not prove a residual source credential causes target OOBE failure.

For a separately created source clone, Microsoft's [Sysinternals Autologon
documentation](https://learn.microsoft.com/en-us/sysinternals/downloads/autologon)
provides Disable as the supported way to turn off automatic logon. Its
[autologon configuration article](https://learn.microsoft.com/en-us/troubleshoot/windows-server/user-profiles-and-logon/turn-on-automatic-logon)
explains the distinction between Winlogon plaintext and an LSA secret.
Microsoft's [unattended AutoLogon reference](https://learn.microsoft.com/en-us/windows-hardware/customize/desktop/unattend/microsoft-windows-shell-setup-autologon)
warns that explicitly enabling Administrator or configuring Audit autologon can
prevent OOBE; normal Audit Mode already enables and logs in that account.
A preparation experiment should use that supported workflow on a new clone,
then run Microsoft Sysprep and freeze new evidence. It must not alter or boot the
current frozen source, or treat the experiment as a proven fix before fresh Setup.

The seven V6 metadata diagnostic artifacts listed by
`v6-offline-diagnostic-metadata.json` were re-read and matched their recorded
SHA-256 values during this audit. V6 readiness-second and V7 readiness-first
were also inspected directly; both report Ready=false, IMAGE_STATE_COMPLETE,
zero setup flags, and an empty desktop array.

## Full-Windows apply-stage control

`full-windows-dism-apply-stage-control.json` now establishes that Microsoft
DISM 26100.7019 on Windows kernel 26100.8036 applied the current native WIM
successfully with /EA before any target boot. All nine fixture entries and
331 streams match, including all 64 object-ID bytes with nonzero birth fields.
This supplies current raw evidence beyond the unre-auditable historical tiny
apply summary. The never-booted target contains all 5,727 selected source paths,
but zero process-trust ACE20 and zero resource-attribute ACE18 paths; apply-stage
reserved SACL preservation fails.

This control does not establish preservation by Setup's WinPE 26100.1742 engine,
nor locate the exact phase of V6's later object-ID birth-field loss. Storage-class
and sparse allocation FSCTL gates are outside this observation. Regeneration of
some process-trust labels after apply remains an inference, not an operation
trace or a waiver. Fixture metadata, comparison, reserved-SACL inventory, engine
version and result artifacts were re-read and matched their recorded hashes.

The diagnostic examples accept BACKING=- for a disk without authorized parents,
and skip recovery-partition inspection only when partition 4 is absent. A present
partition 4 still uses strict partition selection and NTFS open; failures propagate.
These changes enable isolated apply diagnostics without relaxing production
capture's required WinRE policy or parser validation.
