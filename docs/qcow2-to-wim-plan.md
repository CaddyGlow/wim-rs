# Native Rust QCOW2 to Windows installation WIM plan

## Objective and acceptance boundary

Capture the Windows NTFS partition of an immutable, shut-down QCOW2 disk into a
Windows Setup-compatible `install.wim`, entirely through Rust libraries. Assemble
that image into installation media using the existing media and ISO code, then
prove installation on a fresh disposable VM. Production conversion must not
invoke QEMU disk tools, DISM, upstream C wimlib, libguestfs, or mounted NTFS
filesystem services. Those tools may provide independent test oracles; QEMU may
run installation tests.

The native disk, partition, NTFS, volume-capture, and captured-media routes are
implemented. Compatibility still requires the independent metadata, recovery,
and fresh installation gates below. The [feature ledger](qcow2-capture-status.md)
tracks supported profiles, explicit rejections, and retained evidence.
Sysprep remains a Windows preparation step: customize in Audit Mode, run
`sysprep /generalize /oobe /shutdown`, and preserve the resulting disk without
booting its Windows installation again. Native capture does not generalize an
installation. Registry state is supporting evidence, not proof that Sysprep
completed successfully; retain its logs and preparation provenance.

The output captures one Windows filesystem, not a sector clone. GPT, EFI, MSR,
and recovery partitions are not bundled into that image. Windows Setup creates
the destination layout. Recovery is required for the initial installation-media
profile: read the intended `winre.wim` from the source Windows or recovery
partition, or accept an explicit compatible recovery image, and include it in
the installation WIM. The destination recovery partition and registration must
be created by the validated Setup workflow; source partition identities must
not be carried over as destination recovery locations.

## Existing code and gaps

| Component | Current implementation | Required extension |
| --- | --- | --- |
| Safe capture API | `crates/wim/src/api.rs`: directory capture and optional `disk-capture` feature exposing `capture_ntfs` with `VolumeCaptureOptions` | Whole-image and fresh installation gates |
| Capture graph | `crates/wim/src/engine/capture/model.rs`: owned metadata and deferred file/inline/volume streams | Independent whole-image verification and destination metadata checks |
| Windows scanner | `crates/wim/src/engine/capture/windows.rs` captures security, selected NTFS metadata, and data streams through no-follow backup handles, including reparse-owned ADS | Native offline NTFS capture and whole-installation validation; see the [host metadata evidence](wimlib/evidence/windows-capture-full-20261005/README.md) for the tested feature matrix |
| WIM output | Existing writer, codecs, verification, image properties, and export | Verify metadata and resource behavior for whole Windows captures |
| Media assembly | `crates/windows-uup/src/captured_media.rs`: native compatible Setup staging, verified install/WinRE payloads, and ISO publication | Fresh Setup, recovery registration, and WinRE boot |
| QCOW2/partition/NTFS reads | `crates/windows-disk`: raw/QCOW2 chains, deflate/zstd, exact allocation validation, GPT/basic MBR; `crates/windows-ntfs`: native logical streams and metadata | Wider malformed-input coverage and unsupported-feature extensions |
| Image identity/recovery | `crates/windows-uup/src/capture_disk.rs`: offline hives/kernel, source hashes, observed alias provenance, WinRE and unbound ReAgent configuration | Independent destination identity, servicing, and recovery gates |

Align with [the existing WIM implementation plan](wimlib-rust-implementation-plan.md).
Keep native volume parsing separate from host-directory scanning. Copying a
Linux-mounted Windows directory into a WIM is not a metadata-preservation gate.

## Architecture

The implemented boundaries are `windows-disk` for positional raw/QCOW2 and
partition reads, `windows-ntfs` for filesystem interpretation, the WIM
volume-capture adapter, and `windows-uup` orchestration. Dependency and license
notes for the NTFS parser and codecs are retained in the crate README.

Use an immutable `ReadAt` abstraction with length and exact positional reads.
Every offset, length, multiplication, and range must be checked. A partition
view translates bounded volume offsets to disk offsets. NTFS stream readers
translate logical stream offsets through validated runlists. The capture graph
owns shared source handles so writing never reopens an untrusted pathname or
depends on a temporary mount. Extend `CapturedSource` with a volume stream source
and adapt checksum/write paths; do not duplicate the WIM serialization engine.

Use typed library errors with container, partition, record, attribute, and stream
context; use `anyhow` at the CLI boundary. Put configurable limits on caches,
metadata, recursion, decompression, and total work. Keep reads lazy and memory
bounded; do not stage the complete disk or Windows tree in RAM. Keep unsafe
operations localized and explicit. Public APIs must document source lifetime,
immutability, cancellation, and supported filesystem features.

## Phase 1: inventory, fixtures, and contracts

1. Inventory candidate Rust QCOW2 and NTFS implementations, licensing,
   maintenance, unsafe boundaries, and actual feature coverage. Reuse suitable
   Rust code after tests establish its contracts; identify remaining work.
2. Freeze the initial input profile: raw disks and QCOW2 v2/v3, GPT and MBR basic
   disks, one explicitly selected Windows NTFS partition, and unencrypted clean
   volumes. Support common Windows sparse/compressed streams before claiming
   whole-installation coverage. Reject unsupported encryption, dynamic disks,
   Storage Spaces, and unimplemented QCOW2 features explicitly.
3. Record disk-chain hashes, virtual sizes, partition identity, volume serial,
   Windows architecture/build/edition, preparation logs, fixture provenance,
   and expected capture exclusions. Use small synthetic fixtures plus a pinned
   generalized Windows reference image; do not commit large guest disks.
4. Define a feature/evidence ledger with required behavior, fixture, oracle,
   status, and failure policy. Unknown mandatory features must fail closed.

Exit: agreed interfaces and fixtures; no claim that existing WIM tests cover disk
capture. Start with a raw-volume vertical slice before implementing all QCOW2
features so NTFS and WIM integration can be tested independently.

## Phase 2: immutable disk and partition access

Implement raw positional reads, QCOW2 header validation, L1/L2 mapping,
unallocated/zero clusters, and bounded backing-chain resolution. Backing paths
must be explicitly authorized inputs, with cycle/depth limits and recorded
identity; never follow arbitrary embedded paths silently. Handle compressed
clusters only for implemented codecs and reject unsupported feature bits,
external data files, encryption, extended mappings, or inconsistent images until
their support is proved. Do not interpret active internal snapshots as the
current disk; snapshot selection is a separate explicit feature.

Validate GPT headers/table CRCs, primary/backup consistency, partition bounds,
and overlap; parse MBR and either implement bounded EBR traversal or reject
extended layouts. Enumerate partitions and require selection when multiple
Windows candidates exist. Reject hibernated, dirty, or changing inputs without
repairing them. Require an immutable snapshot or equivalent exclusive source
arrangement: hashes and advisory file locks alone cannot prevent concurrent writes.

Gate: randomized range reads match `qemu-img` reference output for raw, sparse,
zero, compressed, and multi-level backing fixtures. Malformed offsets, loops,
truncation, unsupported features, and partition corruption yield bounded errors.
Hash source artifacts before and after tests to prove no mutation.

## Phase 3: native NTFS traversal and stream reads

Implement boot-sector validation, MFT record/update-sequence fixups, attribute
lists and extension records, resident/nonresident attributes, validated runlists,
directory indexes, and NTFS name comparison using the volume's upcase table.
Retain names as UTF-16 code units. Resolve security IDs through `$Secure` and
preserve raw self-relative descriptors, including owner/group/DACL/SACL.

Preserve file identity and hard-link groups independently from content hashes;
read unnamed and named `$DATA` streams, sparse ranges, initialized lengths, and
NTFS compression units. Support the required compression codecs or reject the
affected stream explicitly. Preserve timestamps, DOS attributes, short names,
reparse data, extended attributes, and WIM-representable tagged metadata. Never
follow a junction or symlink outside the captured volume. Detect WOF-backed
content: reconstruct its logical bytes using supported providers, or reject it;
do not archive only a backing placeholder.

Define reparse relocation explicitly using the existing
`capture/reparse.rs` machinery where applicable. Ordinary symlinks and junctions
are captured without dereferencing. Resolve source drive-letter and volume-GUID
aliases from offline Windows metadata against the selected volume. Normalize
absolute targets inside the captured tree to the WIM image root, preserve
relative targets, and encode the per-entry fixup state and archive
`WIM_HDR_FLAG_RP_FIX` consistently with the independent capture oracle.
Never rewrite an external target as though it were inside the image. Record
external targets and reject them in the initial installation profile unless a
documented policy explicitly permits retaining them. Reject ambiguous aliases
and malformed supported link payloads. Unknown reparse tags require a documented
preservation or rejection decision. When materializing WOF logical content,
remove or translate its backing reparse/stream metadata consistently so the
applied file does not reference missing source backing storage.

EFS and BitLocker are initially rejected with actionable diagnostics. Maintain a
documented matrix for other metadata: preserve what the WIM format represents,
reconstruct logical content when appropriate, and fail rather than silently
discard required semantics. Allocation layout, USN history, and filesystem
journals are not installation-image payloads.

Gate: compare logical stream bytes and canonical metadata manifests with an
independent Windows reader. Include ADS, hard links, sparse/compressed data,
attribute-list spillover, unusual UTF-16 names, ACL/SACL, short names, reparses,
WOF cases, and corrupted records. Fuzz parser boundaries with allocation limits.
Apply link fixtures onto a different drive letter and volume identity using
DISM and independently verify internal absolute targets, relative targets,
external-target rejection, and WOF file readability. Matching raw source reparse
bytes alone is not this gate; compare the intended relocated semantics.

## Phase 4: WIM integration and complete capture policy

Add a typed volume-capture API feeding the existing capture graph and writer.
Expose image name/description, Windows capture exclusions, strict metadata
policy, resource limits, and cancellation. Exclusions must be versioned and
auditable: define treatment of page/swap/hibernation files and transient data;
never broadly exclude directories containing required Windows components.
Fix host Windows ADS capture as a separately testable change where needed for
parity and oracle comparisons.

Populate Windows Setup metadata from independently inspected offline registry
hives and image files: architecture, edition flags, version/build, languages,
installation type, and system root. Validate required XML properties against a
DISM-captured image from the identical frozen source. Reject contradictory
identity or incomplete inspection rather than inventing defaults.

Resolve WinRE before finalizing the installation image. Inspect
`Windows/System32/Recovery/winre.wim` and offline recovery configuration; if the
active image lives on a separate recovery partition, read that NTFS partition
through the same native disk reader. Permit `--recovery-partition <id>` to resolve
ambiguous candidates, or `--winre <path>` to supply an explicit image; reject
conflicting selectors. Auto-selection requires one unambiguous compatible
candidate. Missing, unsupported, or contradictory recovery sources fail capture
for this profile. Verify the recovery WIM and its architecture/build compatibility,
record its source partition/path and hash, and inject its verified bytes at
`Windows/System32/Recovery/winre.wim` in the capture graph before writing.
Define treatment of the source `ReAgent.xml` and other recovery configuration
using the reference Setup workflow: clear or regenerate source-specific location
bindings as required, and prove destination registration in Phase 6. Do not
reuse the original recovery partition GUID or offset as a destination binding.

Initially emit LZX `install.wim`. Add solid LZMS `install.esd` only after export,
independent reader compatibility, and installation gates pass; a filename change
or selecting LZMS alone is not sufficient evidence of ESD compatibility.

Write to a new temporary destination, verify it, then publish atomically where
the destination filesystem permits. Never overwrite input media or a completed
output implicitly. Cancellation and failures retain the job journal and useful
diagnostics and never expose a partial output as completed. Record selected
partition, chain hashes, exclusions, metadata capability results, and output hash.

Gate: native and DISM/upstream-wimlib captures of the same frozen partition agree
on included files, logical stream hashes, hard-link topology, permissions, and
representable metadata. Compare semantics, not compressed archive bytes. Verify
using an independent WIM reader and apply onto NTFS with DISM for metadata checks.

## Phase 5: application command and installation media

Implemented commands (authorize each backing file explicitly when present):

```text
windows-uup capture-disk --input reference.qcow2 --partition <id> \
    --output install.wim --name "Custom Windows" --job <directory>
# For an ambiguous recovery source, add --recovery-partition <id>;
# alternatively supply --winre <path> for an explicit compatible image.
windows-uup build-captured-iso --install-image install.wim \
    --setup-media <directory> --output custom.iso --job <directory>
```

Use `--backing-file <path>` for each authorized parent. Source aliases supplied
with `--verified-volume-alias` require `--alias-provenance` binding them to the
frozen disk and selected partition. Both commands accept cooperative cancellation
and user-tightened budgets; see [capture controls](capture-controls.md).

For a fresh Windows Setup installation whose destination system drive is C:,
add `--setup-system-drive C:` to capture. This requires independently verified
source C: alias evidence and records `WINDOWS_UUP/CAPTURE/SETUP_SYSTEM_DRIVE` in
WIM XML. Internal absolute links retain C: with `NOT_FIXED` so Setup's temporary
NewOS staging path does not become their final target. The default capture API
continues to relocate links to an arbitrary apply destination. Media assembly
rejects portable staging fixups for absolute Windows links; the explicit C:
profile still requires an actual installation and working-link gate.

`Wim::capture_ntfs_with_audit` reports kernel-managed EA omissions, storage-class
nodes, sparse files, and hole ranges from the actual capture traversal. The
manifest retains kernel EAs; portable output omits only `$KERNEL.` records under
Microsoft's /EA policy. Sparse TAG3 and storage-class TAG4 have independent
Microsoft apply evidence. Process trust labels must be checked after Setup:
the earlier isolated DISM apply did not restore its checked trust label, while
the fifth Setup target restored the checked AV1 label. Neither observation
establishes whole-label coverage. The recovered V6 installation loses reserved
ACEs at 216 existing source paths and loses extended object-ID information;
its full metadata gate fails. Separate apply-before-first-boot controls must
identify whether these losses occur during apply or later specialization.
Whole-label coverage remains required. The subsequent V14 UEFI/TPM installation reaches OOBE and the desktop, passes DISM/SFC and representative servicing with reboot, and boots WinRE from the new destination recovery partition. Full installed metadata fidelity still fails on aliases, Object IDs, security, hardlinks and recovery access timestamps. Retained independent comparisons for the exact V14 WIM preserve all 101,168 selected source aliases, 601 required empty POSIX short-name fields, 24 full Object IDs and 5,732 reserved raw security descriptors; these selected differences therefore arise after capture. Application versus later Setup/boot attribution and the remaining phase 6 gates are still open. The original source leaf is currently missing, so renewed complete source audits require its recovery. See the [current evidence ledger](qcow2-capture-status.md).


Implement a separate native captured-image media path that runs on Linux and
Windows. It accepts a complete, already bootable Setup media directory and stages
it in an isolated job; it must not call `prepare_base_media()`,
`prepare_boot_images()`, DISM, or Windows-only servicing. Reuse portable ISO and
copying primitives after checking their actual platform dependencies. Preserve
the supplied `boot.wim`, Setup binaries, and BIOS/UEFI boot assets, replace the
installation payload, and remove competing staged `install.wim`, `install.esd`,
and split `install*.swm` payloads. Retain source media and record all staged
changes. Fail if complete compatible Setup media is unavailable; reconstructing
Setup boot images is outside this path.

Make partition inspection available before capture. Validate compatible Setup
architecture/version and correct edition selection; avoid stale `ei.cfg`, PID,
answer-file, or multi-index assumptions inherited from source media. Preserve
BIOS/UEFI boot assets through existing ISO assembly. Verify that the installation
image contains the required WinRE payload. Integrate with retained job evidence
and add job recovery only where
source identity and completed stages can be revalidated.

Gate: CLI integration tests cover ambiguity, existing destinations, changing
sources, unsupported features, cancellation, and correct image/media selection.
Cover WinRE in the Windows partition, WinRE on a separate recovery partition,
explicit recovery input, missing/incompatible recovery images, ambiguous
candidates, and stale source recovery bindings. Run the entire capture-to-ISO
path on Linux with QEMU disk tools, DISM, upstream wimlib, libguestfs, and mounted
NTFS services unavailable; assert that no production stage invokes them. Use
QEMU and Windows tools only in the separate independent validation harness.
ISO structural verification follows [ISO-VALIDATION.md](../ISO-VALIDATION.md).

## Phase 6: independent installation and release gate

Use a pinned reference Windows VM customized in Audit Mode with a marker app,
settings, service, and representative filesystem metadata. Generalize and shut
down using Microsoft's Sysprep; freeze disk and logs. Capture through native Rust,
assemble the ISO, and install it onto a new empty disposable disk using Windows
Setup. Test UEFI + TPM; test BIOS separately if it is included in the support
claim. Never boot the source Windows installation during conversion.

Retain source/chain hashes, preparation and conversion journals, independent WIM
inspection, ISO boot checks, Setup logs, and guest observations. Verify:

- Setup applies the correct edition and reaches OOBE successfully.
- A new machine identity is established; intended apps/settings/services survive.
- Stream bytes, hard-link relationships, reparse behavior, and effective Windows
  permissions match the reference expectation after applying the image, allowing
  explicitly documented setup transformations.
- The component store passes DISM health checks, SFC completes successfully,
  representative servicing succeeds, and the installed system reboots normally.
- WinRE is present, registered to the new destination layout, and can boot.
  Its payload matches the selected source recovery image, allowing documented
  Setup transformations; no source recovery partition binding survives.
- WIM and optional ESD routes each pass independently; source artifacts remain
  unchanged. Follow [COPY-VALIDATION.md](../COPY-VALIDATION.md) where applicable.

Run locked host tests for affected crates, formatting, and
`cargo clippy --all-targets --all-features --locked -- -D warnings` before
submission. Host tests, archive verification, and successful ISO writes do not
replace the installation gate. Update the feature/evidence ledger and existing
audit documents with exactly which Windows builds and input features passed.

## Delivery sequence

Land small reviewable changes in dependency order: contracts/fixtures; raw and
partition reads; NTFS core; metadata and special streams; volume capture/WIM
integration including reparse relocation and WinRE sourcing; QCOW2 mapping/backing
support; portable CLI/media routing; installation
evidence; optional ESD. Parser work includes meaningful corruption tests and
fuzz targets. Each milestone remains experimental until its independent gate
passes. The first release claim is limited to the tested generalized Windows
builds and explicitly supported disk/filesystem features; additional formats and
metadata features require their own ledger entries and evidence.
