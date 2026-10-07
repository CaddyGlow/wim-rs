# Real UUP reconstruction and servicing gate

The run uses a disposable Windows 11 NTFS VM and a read-only optical fixture
containing the real 19045.7727 en-US Professional UUP payload set. The exported
base image is 19041.1. Exact CLI and servicing harness SHA256 values, guest paths,
source payload inventories and completed command results are in `run.json`.
`repaired-run.json` records successful reconstruction, metadata repair, independent
verification, servicing and unchanged hashes for every source payload. No
installed final-target claim is made.

The first SSU applicability query exposed an empty-directory serialization defect:
WIMMount treated `Windows/CbsTemp` as a file when its child-list offset was zero.
The source and exported attribute records both described a directory. Emitting a
terminated child list for empty directory entries fixed the independent DISM
mount regression (`empty-directory-before.json`, `empty-directory-final.json`).
The native empty-image root encoding remains unchanged. The failed private mount
was confirmed against DISM inventory and discarded before retrying; original
state and diagnostics remain in this directory.

The corrected CLI used `prepare-base` on the preserved base exports, rebuilding
boot images and reserializing install metadata. Independent original wimlib
checks passed again, followed by the SSU test in a new private job. This evidence
covers a complete resource reconstruction followed by corrected preparation;
it does not claim a second complete reconstruction with the final binary.

The servicing harness exports a private writable image with the Rust backend,
requires the audited SSU CAB hash and exact package identity, checks applicability,
installs it, commits and remounts, and requires the persisted state to be Installed.
It then verifies the WIM with the Rust backend, checks source hashes and requires
no retained mounts or hives. This is one SSU gate, not complete update closure.

`media-verification.json` records successful original-wimlib verification of the
boot, install and serviced images, ISO generation and hash-matched transfer.
The ISO SHA256 is
`e02e675c7da56df45f6d3c0f8cd77955daf79d531c9dbf70976bb6674091cca6`.
`boot/boot.json` records the exact VM commands and screenshot hashes. Blank BIOS,
UEFI and Secure Boot guests reached Windows Setup language selection; the Secure
Boot console reports `UEFISecureBootEnabled = 0x1`. Screenshots and QMP state are
retained under `boot/`. These are Setup boot checks, not Windows installation.
Host log hashes and test counts are in `host-checks.json`; platform checks apply
to the recorded binaries. All source payload hashes remained unchanged and the
completed servicing job retained no mounts or hives.

Reproduce with `scripts/wimlib/validate-uup-pipeline.py`, passing `--qga-socket`
for a disposable Windows VM, `--cli` and `--servicing-harness` for statically linked
Windows binaries, and `--output` for a new evidence directory.

Interrupted earlier verification and export attempts are retained in the sibling
`uup-pipeline-20261004` and `uup-pipeline-20261004-cached` directories. The
[solid-resource cache description](../../solid-resource-cache.md) explains the
reader and writer changes made after those attempts.
