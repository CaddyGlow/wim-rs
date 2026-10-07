# Baseline reconstruction and media validation, 2026-10-04

The real 47-file local UUP fixture was reconstructed in a disposable Windows 11
NTFS guest from a read-only optical image. The output is Windows 10 Professional,
amd64, en-US, baseline 19041.1. This run validates baseline media and one separate
SSU servicing gate; it does not establish complete update closure, 19045.7727,
or a Windows installation.

The resource reconstruction completed source verification, Setup extraction,
WinRE export and install export. Its final WinRE insertion aborted when append
mode attempted a 6,607,812,592-byte allocation. Original command results,
journals and logs are retained in
[the initial run](../uup-pipeline-current-retry-1004/run.json).
The earlier preflight failure is separately retained in
`../uup-pipeline-current-1004/run.json`.

The writer fix keeps the existing output prefix on disk, buffers only appended
bytes, retains absolute resource offsets and computes integrity from the file.
It removes the full-prefix output clone and its doubling allocation. Existing
compatibility input snapshots are still retained by some mutation paths; this
is not a claim that every writer path uses bounded memory.

The corrected binary successfully ran `prepare-base` on the preserved exports.
The combined evidence covers full resource reconstruction followed by corrected
final preparation, rather than a second complete reconstruction with that binary.
Exact original and corrected CLI/harness hashes, command results, source hashes
and the initial abort are recorded in `run.json` and its linked provenance.

Original Windows wimlib verified the final boot and install WIMs. A private SSU
job installed `Package_for_ServicingStack_7714~31bf3856ad364e35~amd64~~19041.7714.1.3`,
committed and remounted, and required the persisted state to be Installed.
Rust and original wimlib verified that separately serviced image. All 47 source
payload hashes remained unchanged, both job states contain empty mount/hive
lists, and live DISM inventory reported no mounted images.

`media-verification.json` records successful independent WIM checks, baseline
ISO creation and hash-matched host transfer. The output is
`/data/cache/wim-uup-current-1004-validated.iso` (4,528,449,536 bytes), SHA-256:

`042df5114a6ac5c910c9a5e2f1179d9bd1079f6db9d5e67fd4166863faa49094`

The first 1 MiB QGA transfer read timed out, leaving a retained empty file.
Retry with 64 KiB reads transferred the complete ISO and matched the writer's
hash. `iso-7z.txt` records successful independent archive testing of 900 files
and 84 directories, with the documented redundant UDF-anchor tail warning.

`boot/boot.json` retains exact commands, ISO hash and screenshot hashes. Blank
BIOS, UEFI and Secure Boot guests reached Windows Setup language selection.
The Secure Boot registry screenshot reports `UEFISecureBootEnabled = 0x1`.
These checks use the recorded enrolled Microsoft-key firmware policy and do not
establish all physical-machine revocation policies. All disposable boot guests
were stopped. Initial launch failures (socket length and sandbox KVM access)
are identified in the boot manifest; the successful launch used approved KVM
access and a shorter state directory.

`host-checks.json` records passing WIM and windows-uup host tests and affected
crate Clippy checks on Linux and Windows. Regression coverage checks append
buffering at a 3.3 GB absolute offset and multi-resource solid appends with
integrity validation. Workspace-wide Clippy and formatting encountered
unrelated concurrent WSUS module/warning failures, recorded separately.
