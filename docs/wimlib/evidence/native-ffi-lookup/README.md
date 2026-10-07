# Native resource lookup callback API

`wimlib_iterate_lookup_table` lists image metadata in image order, followed by
unique content blobs in unspecified order. It uses the lookup table retained
at open time, makes no heap allocations during traversal, forwards the caller's
context, and stops at the first nonzero callback return. Invalid flags are
rejected before handle access. New empty handles traverse no resources.

The native `repr(C)` resource structure reproduces the unchanged header's
88-byte Linux host layout. Both C clients compare `sizeof` and field offsets.
All ordinary/solid blob fields, six bitfields, hashes, reference counts and
reserved fields are compared, including the raw resource fields populated by
original wimlib for ordinary resources despite their narrower header comments.

`contract-red.log` records the absent native symbols; two regression tests
pass after implementation. `allocation-red.log` records 21 allocations across
three iterations when lookup tables were reparsed. Retaining the table fixes
this; `allocation-green.log` proves zero allocations for repeated traversal.
`differential.json` records 11 unchanged-header client comparisons across all
ordinary/pipable codecs and the three solid codecs. Metadata ordering and
positive/negative callback cancellation are checked separately from sorted
content rows.

Reproduce:

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim
python3 scripts/wimlib/check-lookup-abi.py --native target/debug
cargo test --manifest-path Cargo.toml --target-dir target --locked -p wim --test lookup --test lookup_allocations
```

Current image order follows retained source identities after deletion. Owned
empty images emit synthetic metadata entries with zero reported size, reference
count one, and initially zero hash; successful writing publishes their metadata
digest. File-backed blobs remain visible when their reference counts reach zero.
These behaviors have additional evidence in the image-mutation and C writer
directories.

This export remains partial. Broader mutation/rollback/allocator-failure order,
platform bucket widths, concurrent handle lifetime and adversarial corruption
remain validation gates. Callback entries are borrowed only during the call;
callbacks must not mutate or release the iterated handle.


## Retained bucket order and independent captured references

`buckets-original.txt` is an unchanged-source private-table structural oracle:
50-byte disk entries, native-size digest prefix, head insertion, 64/65/66 and
128/129/130 growth, unlink/reinsert, and failed-growth retention. Four Rust
index tests compare those chains directly. Real bucket heads and links use the
registered allocation strategy; allocation/release occurs outside index borrows.

`ordered-collisions/results.json` records **44/44 literal public C outcomes**.
Real payload SHA-1 digests collide under both 64- and 128-bucket masks; counts
around both growth boundaries exercise loaded capacities including three extra
raw metadata entries and new-handle resource-reference insertion. Every callback
row and cancellation at every row is compared without sorting or normalization.
Fixtures preserve original metadata and existing payload and append real content;
this matrix claims lookup traversal, not extraction of the appended unreferenced
payloads. `order-artifacts.json` identifies caller and frozen library hashes.

`captured-reference/{original,native}.txt` compare the same public C caller:
capture an actual file and inline symlink, hash via image export, reference their
resources into an image-less handle, free both source/export handles, and verify
the independent resource owners. Ordered rows match; verify succeeds, changing
the captured file returns 88, and restoring it allows retry. Native references
retain the actual stream Arc, not a fake decoded empty buffer. Extraction,
verification and writer source dispatch distinguish that owner explicitly.

The writer and generated-image matrices are maintained by the writer agent.
They test callback-time hash publication separately. The latest writer traversal
has 360 matching descriptor rows (the earlier 52 order differences remain
preserved in its red evidence), and 288 generated-image API rows match original.
Broader lifecycle events and mutation families remain separate gates.

`captured-reference/writer-{original,native}.txt` additionally import metadata
from a written/reopened archive so the destination uses its independently
referenced captured owners for writing. Source/export/archive handles are freed.
Write succeeds, changed and truncated sources both return 88 on verify/write,
and restoration permits retry. `independent-verify.txt` is the unchanged original
reader verifying the native retry archive. The actual caller and output hashes
are in `order-artifacts.json`.

`captured-mutation/{original,native}.txt` literally match public capture inline
publication, export-triggered deferred hashing, rename without reinsert, delete
of an inline link, image deletion retiring remaining captured hashes, and re-add
with fresh head insertion. Graph-replacement adapters stage bucket transitions
before commit. The fourth index test reenters traversal from actual registered
allocation/release callbacks during workspace reserve, bucket growth and snapshot
clone/destruction; each observes a complete chain with the published count.

`captured-gift/native-red.txt` preserves missing immediate GIFT unlink of hashed
captured descriptors. The corrected literal source/destination rows match
`original.txt` and `native.txt`, and the destination verifies after source
release. This also observes the gifted source for comparison; the public header
explicitly restricts post-GIFT source use to freeing it, so those source rows
are an additional observation rather than a promised API usage contract.
