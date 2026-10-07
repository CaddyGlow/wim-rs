# Windows extraction implementation contract

This is an implementation plan, not evidence that native Windows extraction
works. The reference is `/tmp/wimlib` commit
`cd5e231c348c255ae5088873b5a66ee0eb96fa07`. The selected capture fixtures already
have independent original-library verification; preserve them as inputs and
extract each run into a separate disposable directory.

## Backend boundaries

Keep resource decoding and hashing in `wim/src/engine/extract/blob.rs`. Its owned
memory, captured-file and archive-backed sources must retain their actual
ownership and read only the requested chunk. The Windows positional-read
adapter must handle short reads, interruption and EOF. Keep path selection in
`extract/paths.rs`; share its selected node/parent mapping once the Windows
backend exists. Do not enable platform gates solely because a module compiles.

The Windows backend should own target directory handles, selected inode sinks,
UTF-16 names, progress text and final metadata operations. Preserve raw UTF-16
name units, including unpaired surrogates. Reject invalid leaf components before
filesystem mutation. Use handle-relative NT operations beneath the opened
target rather than repeatedly resolving image-derived absolute paths. Directory
opens must not traverse an unexpected reparse point. Handle ownership must
release on every cancellation and error path.

## Source sequence and observable progress

`src/win32_apply.c:3272` prepares the target, starts the structure phase, creates
directories and nondirectories, ends that phase, extracts blobs, starts the
metadata phase, applies metadata in reverse dentry order, then ends that phase.
Both Windows file phases count **all selected dentries**. The Unix backend's
directory-plus-empty-file structure count and directory-only metadata count
cannot be copied to Windows.

`src/extract.c:95` and `include/wimlib/apply.h:105` send phase progress initially,
after each 500 actual completed files, and at completion. Advance counters at
the real filesystem operation boundary. `src/extract.c:439` advances stream
progress before calling the sink write operation. Preserve that ordering,
including hardlink and duplicate-stream accounting: cancellation at the first
stream callback leaves the newly created file empty. Verify resource hashes
when the stream finishes. Preserve callback cancellation at each boundary and leave the same
partial output that the source leaves; do not synthesize callbacks after a
failure.

## Ordinary NTFS metadata

## Reparse stream implementation gate

The ordinary backend currently rejects reparse streams. Extend the actual
stream sink rather than translating a reparse inode into a regular file.
`src/win32_apply.c:2013` buffers the complete reparse payload before issuing
FSCTL_SET_REPARSE_POINT; bound this buffer by REPARSE_DATA_MAX_SIZE. Reconstruct
the eight-byte reparse header from the metadata tag, data length and reserved
field. A zero-hash reparse stream is a separate structure-phase operation
(`create_empty_streams()`), with no invented resource or stream progress.

Directory reparse streams must participate in resource selection even though
ordinary directory data streams do not. Keep a stream destination's kind and
inode identity alongside its index; a single blob may feed both data and reparse
destinations. Continue hashing decoded input and reporting progress before sink
writes. At stream completion set the reparse payload through a handle opened
with GENERIC_WRITE and the source's reparse-safe open options. Preserve source
errors SET_REPARSE_DATA and INVALID_REPARSE_DATA. Default mode tolerates only
access/privilege failures for symlink and mount-point tags; STRICT_SYMLINKS must
return the failure. Other tags and failures remain errors.

For RPFIX, retain the actual NT namespace target returned by the existing RTL
path conversion. `try_rpfix()` leaves malformed link buffers and relative
symlinks unchanged. Absolute links whose inode lacks NOT_FIXED replace their
device component with that NT target, preserve the device-relative suffix,
remove duplicate leading separators, and derive the printable name by skipping
the NT top-level component. Do not use UTF-8 round trips or filesystem canonical
resolution: raw UTF-16 names and the source's lexical path rules are required.

Before enabling this gate, retain original and native raw observations for
junctions, absolute/relative file and directory symlinks, empty/oversize buffers,
RPFIX/NORPFIX, strict permission failures, and stream cancellation. Observe
FSCTL_GET_REPARSE_POINT tag and payload after extraction, independently verify
the native WIM with the original library, and compare original extraction of
the same archive. Existing ordinary extraction and live pipe cases must remain
green. Capture support or a successful WIM verify alone does not establish this
apply gate.

Create one physical file per inode and real hardlinks for its aliases. Set DOS
short names using the original conflict/removal policy; strict failures must
return the original error. Restore metadata on children before parents so that
read-only attributes, timestamps and restrictive DACLs do not obstruct later
operations.

`src/win32_apply.c:3150` opens metadata handles with write-attributes,
write-EA, write-DACL, write-owner and system-security access. On permission or
privilege failure, retry after removing system-security, then write-DACL, then
write-owner access. Do not conflate this handle-open policy with security-set
retries.

`src/win32_apply.c:2939` copies each self-relative security descriptor and adds
DACL/SACL AUTO_INHERIT_REQ when the corresponding AUTO_INHERITED bit is set.
Request owner, group, DACL, SACL, label and backup information using
`NtSetSecurityObject`. In non-strict mode, only access/privilege failures permit
successively dropping SACL/label/backup, then DACL, then owner. Strict mode must
report the original set-security error. NO_ACLS skips this operation.

`src/win32_apply.c:3070` sets creation, access and write FILETIMEs together with
attributes using FileBasicInformation. Mask directory, reparse, encrypted,
sparse and compressed bits because these require separate operations. With
NO_ATTRIBUTES, set NORMAL. Preserve the original exceptional root-directory
invalid-parameter handling on FAT. Unsupported object IDs, EAs, encryption,
reparse data and named streams remain explicit incomplete gates; ordinary
fixture success cannot establish those features.

## Differential test progression

1. Preserve the current native Unsupported result and the original successful
   ordinary-fixture extraction, with DLL and caller hashes.
2. Test empty directories/files and nonempty UTF-16 filenames, then actual
   hardlink identity, bytes, FILETIMEs, DOS names and attributes.
3. Compare default ACL, NO_ACLS and protected-DACL fixtures using raw descriptors
   and normalized SDDL; keep all raw manifests. Test disabled privileges and
   STRICT_ACLS separately.
4. Compare progress payloads and callback cancellation at structure, stream and
   metadata boundaries. Add a fixture above 500 entries to exercise throttling.
5. Exercise overwrite, denied access, malformed names, path selection,
   duplicate streams, corrupted input and failed writes. Retain source error
   values and partial-output manifests.
6. Only then enable anonymous-pipe extraction using real CRT descriptors and
   original ownership/read-volume probes. Do not wrap a complete in-memory WIM
   and call it a streaming implementation.

Run each fixture through the original DLL and each native GNU/MSVC DLL with the
same C caller in the owned Windows guest. Host tests and cross-compilation are
prerequisites, not substitutes for those runs. Broader NTFS, original upstream
Windows suites, allocator failures, performance, servicing and installation
remain separate release gates.
