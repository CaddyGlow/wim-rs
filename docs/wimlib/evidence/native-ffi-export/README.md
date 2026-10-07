# Native C image export

`wimlib_export_image` now exports current original, pending-empty, and previously
exported images into an independently owned destination. Metadata keeps original
serialized bytes and source resource descriptors. Exported data retains one
independent `Arc<Vec<u8>>` source-file snapshot, shared among exported blobs, with
original descriptors and lazy decompression. Re-export shares the snapshot. The
source handle can therefore be freed before destination verification/writing;
corrupt file payloads still fail at verification/write rather than prematurely at
export, matching upstream's descriptor-only export behavior.

The handle's stable image identity survives export and re-export. It rejects
same-handle duplication, repeated export of an image, and A-to-B-to-A duplication
with error 87. Independently opened handles remain distinct, even for the same
filename. Export checks metadata availability and destination name collisions,
clones arbitrary XML properties, applies name/description overrides or suppression,
sets WIMBOOT, propagates RPFIX, remaps boot selection, and deduplicates data by
SHA-1 while adding stream references including hard-link aliases. Ordinary and
solid source resource descriptors remain available for unchanged-header lookup
callbacks. Destination state is staged for rollback on failure.

GIFT follows the original ownership side effects: only blobs absent from the
destination are unlinked from the source. Source images remain, and their reads
then fail with missing-resource error 55. A differential failure established that
original GIFT does **not** restore source blobs after later invalid XML/name
failure (`src/export_image.c` moves blobs before XML and only rolls back the
destination). The native implementation preserves this behavior rather than
silently making failed GIFT transactional. The red result is preserved in
[differential-red.json](differential-red.json).
The public GIFT contract says the source is no longer accessed after export
except for freeing it. Source-side diagnostics in this matrix explore the
implementation's state; they do not extend that supported lifecycle or require
replicating assertions reached by using a gifted source again.

`check-export-api.py` compiles `probe-export-api.c` against the unchanged original
header and each library. It tests all 32 public flag combinations with single and
all-image export and five override/collision/invalid-name modes. Three more codec
fixtures cover ordinary/all/pending-empty export, and original corrupted-data and
cyclic-metadata fixtures check deferred error timing. The source corpus includes
hard links and shared data across images. Content callback ordering is unspecified
and normalized only as complete resource-record multisets; other observations
retain their original order.

[differential.json](differential.json) matches all 12,676 observations across 386
cases. After freeing source handles, 271 successfully written destination WIMs
were verified and applied by original wimlib; extracted file names and SHA-256
content trees match corresponding original exports. Six focused Rust tests cover
independent metadata lifetime, stable duplicate identity, destination rollback,
XML/boot flags, invalid flag bits, zero-image same-handle behavior, and shared
root materialization visible after writing another owner. Targeted
strict Clippy passed.

This is partial drop-in evidence. Export snapshots use whole-file buffering;
custom allocator hooks, allocation failure injection and Windows remain unverified.
Pending exported images now share `Arc<Mutex<PendingMetadata>>` ownership: a
root materialized and hashed by any live owner becomes visible to every exported
owner. Locks are released before C callbacks.
[shared-red.log](shared-red.log) records the previous independent-copy mismatch
and [shared-differential.json](shared-differential.json) compares the corrected
unchanged-header A-to-B-to-C scenario: writing A materializes all three owners,
writing B preserves their metadata hash/root/timestamps, and writing C after A/B
release preserves the same metadata. All three emitted metadata resources are
byte-identical within each library, including timestamps; original wimlib verifies
all six emitted WIMs. Each emitted pending-root metadata resource is 128 bytes,
matching the original explicit empty child-list framing; the script enforces this
length and compares native/original metadata byte-for-byte after zeroing only
the three wallclock FILETIME fields at root offset +40 through +64. It derives
the root offset from the aligned security-table length, matching the reader.
Its root-list terminator
and empty child-list terminator are separate. Reproduce with
`check-shared-export-metadata.py`.

Filesystem updates must eventually enforce original shared metadata error 86 and
ownership transitions. Loaded original WIM metadata is currently immutable and
snapshotted; future directory-tree mutation requires shared ownership for that
state too, beyond the verified pending-root lifecycle. External/delta resource referencing,
progress/cancellation, capture-produced unhashed streams, complete source-file
mutation behavior and shared metadata mutation are still outstanding. The apply
comparison verifies paths and payload content, not all platform security, special
file, timestamp, or installation semantics.
