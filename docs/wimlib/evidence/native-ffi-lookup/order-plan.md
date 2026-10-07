# Retained blob traversal order: source-backed implementation gate

The writer post-abort state comparison retains 52 mixed-image lookup order
mismatches. Digests, sizes, references, verification and retry outcomes match;
callback rows still differ in order. Do not sort the evidence to report equality.
The public lookup documentation does not promise an ordering, but retaining the
original observations is a stricter compatibility gate for callers that stop
iteration early.

## Original behavior

Reference `src/blob_table.c:59–84,294–382,889–902,1383–1410` and
`src/wim.c:190,757` in the preserved checkout:

- A new archive starts with 64 buckets. A loaded archive starts with the next
  power of two of the raw disk lookup entry count, including metadata entries.
- The bucket is the native `size_t` prefix of the SHA-1 digest, loaded unaligned,
  masked by capacity minus one. Each insertion adds at the bucket head.
- Insertion grows only when the previous blob count exceeds the mask. Growth
  walks old buckets in ascending order and reinserts at the new bucket heads,
  reversing each resulting chain as the source does. Failed growth retains the
  old table rather than failing the insertion.
- Public iteration emits each image's metadata followed immediately by that
  image's unhashed streams, then the global hashed table in bucket/chain order.
- Hash readiness is observable during callbacks: unique raw streams aborted at
  DONE_WITH_FILE retain a zero hash; completed compressed/solid and prehashed
  size-collision streams already expose their digest. The writer state evidence
  exercises these boundaries directly.

## Native representation and integration

Add a retained order index to the handle, independently of descriptor ownership.
Use allocator-backed bucket heads and links; each link identifies a real digest
and owner, not a copied or fabricated resource descriptor. Keep the capacity,
chain order and insertion history authoritative. Record insert/unlink transitions
where the real blob enters or leaves hashed ownership, including capture inline
reparse data, deferred writer publication, deduplication, exports, references,
image deletion, template reuse and overwrite. A final sort of current hashes
cannot reproduce insertion history or source rehash reversals.

Construct the index from actual disk entries before callback-visible handle
publication. New handles allocate the real 64-bucket storage through the active
hook; preserve recoverable ownership on each allocation failure. Growth must be
fallible and retain the prior index on failure. Checkpoint/rollback must restore
this index with the image and blob state, with no locks or whole-handle borrow
held over C callbacks. Referenced descriptors remain owned by their existing
retained backing objects. The order index must not extend a dead caller pointer.

## TDD sequence

1. Preserve the current 360 writer-state records and their 52 ordered differences
   as the integration red. Re-run with a frozen native artifact for each change.
2. Add an unchanged-header C lookup probe for new and reopened archives, multiple
   images and unhashed/hashed mixtures. Capture every row and early-stop result.
3. Test deterministic digests that collide in the low native-word bits, capacities
   around 64/65/66/128/129/130, deletion/reinsertion, failed growth, and reopen
   capacities determined by raw entries rather than retained content count.
4. Unit-test bucket chains and growth against source-derived expectations, then
   integrate the actual writer readiness transitions. Never publish every staged
   hash at callback 26: the raw-stream abort case would regress.
5. Run source comparisons for export/reference/template/update rollback and
   overwrite, including callbacks that stop at each metadata/content row. Keep
   platform `size_t` width and allocator failure records scoped separately.
6. Re-run the portable upstream scripts and public lookup/capture/writer evidence.
   Close only proven order gates; archive encoding-size differences remain
   separate from hash ownership and traversal behavior.
