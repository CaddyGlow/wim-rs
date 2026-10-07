# Native QCOW2 Windows capture evidence — 2026-10-05

The frozen, generalized Windows 11 Pro 25H2 source was captured through Rust
QCOW2, partition, NTFS and WIM libraries with `PATH=/nonexistent`. Production
capture and media assembly invoked no external conversion tools. QEMU,
Windows and upstream wimlib are independent validation tools only.

## Completed gates

- [Source preparation](source-preparation.json) and [generalization](source-generalization.json):
  Sysprep resealed to OOBE and shut down. The source was not booted afterwards.
  The retained source BCD firmware-export warning remains part of the evidence;
  the destination must obtain its own EFI/BCD through Setup.
- [QCOW2 allocation audit](source-qcow2-allocation-audit.json): both explicitly
  authorized containers pass exact allocation/refcount accounting. Independent
  [QEMU checks](qemu-check-oracle.json) report no check errors.
- [Native metadata differential](offline-fixture-differential.json): nine fixture
  entries and 331 streams match the Windows observations. EA/object-ID baseline
  coverage is limited to the five files independently observed for those fields.
- [Capture report](capture-report.json): 176,416 entries, exact verified WinRE,
  unchanged physical source/backing hashes, no warnings, and published WIM hash.
- [Independent WIM oracle](independent-wim-oracle.json): every resource verifies,
  WinRE verifies, image identity and fixture topology match, and metadata reads
  are warning-free. Linux extraction cannot restore all Windows attributes;
  those extraction limitations are retained separately from reader warnings.
- [Native media report](media-report.json): one installation image, verified
  Setup boot index and embedded WinRE, with the published ISO hash.
- [Prior host validation](host-validation.json): 2,175 tests passed before the latest fixes;
  the ignored tests require their separate platform gates. That full workspace
  Clippy run passed. [Earlier incomplete attempts](host-validation-pending.json) retain interrupted compilation/doctests. [Current host checks](host-validation-portable-metadata.json) passed 2,306 tests (49 ignored), formatting, and full all-target/all-feature Clippy after the portable metadata and media guard fixes. These checks do not prove Windows installation.
- [QCOW2 fuzz smoke](qcow2-fuzz-smoke.json): short bounded instrumented campaigns
  passed; this is not exhaustive malformed-input coverage.
- [Native NTFS fuzz smoke](ntfs-fuzz-smoke.json): independently formatted seed
  reaches manifest traversal; mutation tests and 1,001 instrumented iterations
  passed. Memory/work bounds and limited coverage are recorded.

The prior retained full-workspace run includes the completed cancellation and budget
APIs and final publication checks. Their focused platform regressions are linked
from the [capture controls](../../../capture-controls.md).

The [superseded independent result](independent-wim-oracle.superseded.json)
retains the first WIM's reparse-directory child-offset warnings. The fourth attempt’s
capture used WIM SHA-256
`31e22fe2b8a79dbcf467e81b7c5f001e63caae0d26d14445fc37c61386de46c0`.
That image included the complete recovery schema and passed the then-current 16
independent oracle gates. Its native ISO was published without warnings and fed
the fourth fresh installation, which passed image application and offline specialization but failed OS relocation.
The fourth attempt’s native ISO SHA-256 is
`e432cc849bbb242a8e816b6ba87561065f728fa1def44b43255d9b205a4ce199`.

## Pending destination gates

Disposable, blank 64 GiB disks have been tested through standard Windows Setup
with Secure Boot and TPM 2.0, using the verified native ISOs. The first Setup attempt failed after reaching 46%; its stopped VM, logs and
image evidence are preserved. A deferred reparse-stream normalization fix
now passes raw layout checks for all 73 reparses. The corrected image is being
tested through fresh Setup attempts, with VM storage on `/data/cache`. The second attempt applied the WIM successfully, then failed on cached Panther
answer-file staging. The third attempt excluded exactly those two cached answers
and passed the absence preflight and image application, then failed offline WinRE
specialization because generated recovery XML omitted required schema fields.
The [third attempt](installation-third-attempt.json) and
[causal diagnosis](reagent-schema-failure-diagnosis.json) preserve this failure.
The [fourth attempt](installation-fourth-attempt.json) passed offline specialization with the complete recovery schema, then failed because captured root `Windows.old` collided with Setup’s relocation destination (`0x800700B7`). The capture policy now excludes this exact root directory; nested names and `.bak` remain captured. The independent absence regression rejects the fourth image. The fifth input WIM is published with this correction and cancellation wrappers that preserve shared hard-link stream identity. Its native verification and all 17 independent oracle gates pass; all three input hashes are unchanged. The fifth native ISO is published without warnings; its SHA-256 is `e40faabad2edbe78d089684bcf65aa0bf29f25681d1008e9f7bb0fc39e595f2d`. The fifth blank-disk attempt has booted Windows and reached first-boot setup (42% observed); OOBE failed on the captured disabled CloudExperienceHost task. A separate source-preparation clone is being corrected through supported task enablement and Microsoft Sysprep; the original remains frozen. Installed runtime gates remain pending. See [its capture report](capture-report.explicit-fifth-input.json).
The explicit WinRE route and selected recovery partition route both pass native capture and all 17 independent oracle gates. The selected route uses a disposable three-container fixture; its Windows partition is inherited unchanged from the frozen source, and all fixture/source hashes are preserved. See [selected capture](capture-report.selected-recovery-positive.json) and [independent verification](independent-wim-oracle.selected-recovery-positive.json).
The fifth installed-system observer found Object ID birth-field loss, a cleared sparse attribute and a custom junction pointing into the relocated Setup staging directory. Recovery registration uses new destination identity, but location and payload comparisons fail the current predicates. Independent DISM application is investigating these differences; installed metadata and recovery are not passing gates.
Installation completion, installed metadata/identity, servicing,
reboot health, recovery registration and actual WinRE boot remain pending. A bootable ISO and host tests alone do not
establish these gates.

Large disks, WIM/ISO artifacts and raw oracle logs remain under
`/data/cache/qcow2-capture-20261005` and
`/data/cache/qcow2-installation-1005`; they are not committed.

## Supported boundary

See the [feature ledger](../../../qcow2-capture-status.md). Internal snapshots,
persistent bitmaps, extended L2, external data and QCOW2 encryption are rejected.
NTFS encryption, unsupported storage/reparse providers and case-sensitive
directories are rejected. Caller-provided source immutability is mandatory;
matching before/after hashes do not provide exclusive snapshot isolation.

Current [portable metadata gates](portable-metadata-policy-gates.json) and
[independent Microsoft apply evidence](portable-metadata-apply-and-trust-probe.json)
prove sparse allocation, desired storage class, full Object IDs, logical streams,
and exact retained user EAs on a fresh isolated NTFS volume. The current Setup
profile requires verified C: aliases and retained C: absolute targets. A complete
new installation is still required. Bare DISM does not restore reserved process
trust labels; actual fifth Setup evidence shows the checked AV1 label restored,
with whole-label coverage under investigation.
