# Native C image addition and deletion

`wimlib_add_empty_image` and `wimlib_delete_image` now change real owned image
state. `WimHandle.images` keeps current order through `HandleImage::Source`
(original one-based lazy metadata identity) or `HandleImage::Empty` (owned
serialized security table and null directory root shared across exported owners). New empty metadata is 16
bytes: an eight-byte empty security table followed by an eight-byte null-root
marker, independently accepted by the native metadata parser. Its pending digest
starts zero. `Arc<Mutex<PendingMetadata>>` now shares root materialization and
digest with exported owners; see the export shared-metadata differential evidence.
The writer materializes and hashes the directory root when writing,
matching the original library's distinction between added and written images.

The XML entry records zero initial counts and Windows creation/modification
timestamps. Raw POSIX image names remain bytes, including invalid UTF-8; XML
serialization subsequently reports error 31. Invalid controls reject the append,
name collisions report 11, and failed addition preserves optional index output.
Image count, owned image order, XML numbering and boot selection change together.
As the original APIs do, addition/deletion do not themselves reject marked-readonly
handles. Images missing metadata return 36. Deletion loads and validates source
metadata lazily and maps metadata hash failure to 21. All-image deletion proceeds
in reverse order, including its original partial-change behavior on later failure.

Deleting source image streams decrements the original retained data descriptors
with saturation. A differential failure exposed an initially incorrect removal
of zero-reference blobs: original `src/blob_table.c:226` explicitly retains
`BLOB_IN_WIM` even at reference count zero because persisted counts are untrusted.
The corrected implementation preserves those descriptors. `removed_blobs` is
reserved for future non-WIM-backed content and stays empty for these APIs.

Three focused Rust tests cover real rootless metadata ownership, append failure
transactionality, and deletion/boot/XML consistency. The unchanged-header C probe
also compares every intermediate image/property/resource snapshot, collision,
invalid/raw name, readonly, invalid index, boot remapping and repeated delete-all
behavior. `scripts/wimlib/check-image-mutation-api.py` runs a new empty handle,
four original three-image codec captures with shared content, and a synthetic later split part missing image metadata, and seven original
regression WIM fixtures (including corrupt/cyclic metadata).
[differential.json](differential.json) matches all 4932 observations across 13
cases. Resource traversal order is unspecified and normalized as complete
record multisets within each snapshot; other lines retain their original order.
[differential-red.json](differential-red.json) preserves the zero-reference
retention regression before its fix. `native-new-red.log` additionally captures
the pending empty-descriptor lookup integration gap before root's adapter landed.

This is partial drop-in evidence. The metadata model supports original backed
images and newly empty images; filesystem capture/update, shared export ownership,
custom allocator failure behavior, concurrent operations, callback mutation,
Windows names/security, streaming writes and complete lifecycle validation remain
outstanding. Separate native C writer evidence covers emitted WIM interoperability.
