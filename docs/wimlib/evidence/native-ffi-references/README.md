# Native external resource reference C APIs

`wimlib_reference_resources` and `wimlib_reference_resource_files` now reference
real file-data descriptors without importing image metadata, changing XML/header
identity, or increasing pre-existing blob reference counts. New descriptors retain
the original source counts, ordinary/solid resource locations, compression settings
and source part number. SHA-1 identity deduplicates both repeated source handles
and already present destination blobs. Self-reference is a no-op. The complete
handle array is validated before adding any descriptors.

Each needed opened source file is retained as an independently owned lazy
`Arc<Vec<u8>>` snapshot, shared by all referenced blobs from that source. File
payload decompression and SHA-1 checking remain deferred until verification or
writing. Source handles can be released immediately; destination operations do
not borrow the source handle or depend on its lifetime. Only data blobs are
referenced; source metadata records and image identities remain excluded.

Both functions stage additions and roll back every newly added descriptor when
later opening, globbing, or allocation fails. File opens use the destination's
registered progress callback/context through `wimlib_open_wim_with_progress`.
Staging holds a shared destination reference so callback registration replacement
through its `Cell` does not alias a live mutable handle reference; exclusive
mutation is acquired only for final commit. Registration is snapshotted separately
for each source file, matching upstream. Integrity callback continue, abort (76),
invalid status (77), and replacement during the first file are independently
compared with original wimlib.

Unix glob expansion uses the OS's POSIX `glob` API through pinned `libc`, with
`GLOB_ERR | GLOB_NOSORT` like the original library. Flag 1 enables globs; flag 2
makes unmatched enabled globs report 8. Without flag 2, an unmatched pattern is
retried as a literal path and normally reports open error 47. Handle references
accept both public flag bits but ignore them, as upstream does. Zero-count file
references do not evaluate open flags or paths. No original wimlib C code is linked
into production.

The unchanged-header probe initially failed to link the absent native APIs;
[link-red.log](link-red.log) preserves that red state. Three Rust regression tests
cover independent source-free lifetime and data-only import, complete-array null
validation before mutation, and rollback followed by successful file reference
and verification after input unlink. Targeted strict Clippy passes.

`check-reference-api.py` compares complete intermediate callback records and
return codes for handle/file references, duplicates, self references, missing
parts, bad flags, later-part destinations, invalid/missing files, glob matching
and nonmatching, rollback, integrity opens and callback control/replacement.
Its original four-part split has missing data before reference and reconstructs
an unsplit WIM after supplying the remaining parts. Additional XPRESS, LZX, LZMS
and solid-LZMS fixtures produce delta WIMs by retaining only metadata lookup
records; referencing their original source supplies the real missing compressed
resources. This transformation preserves source files and runs inside disposable
test directories.

[differential.json](differential.json) matches all 823 observations across 62
cases. After source-handle release, 29 emitted native WIMs were verified and
applied by original wimlib; extracted paths and SHA-256 payload trees match the
corresponding original result. Unspecified content traversal order is normalized
only as full record multisets within each snapshot. The existing original SIMD
oracle issue is handled by explicitly disabling `sse4.2`, as recorded in the JSON.

Exploratory GIFT restoration is outside the original API's lifecycle contract:
`wimlib.h` says GIFT should be used when the source will not be accessed after
export except for `wimlib_free`. The preserved
[original-gift-restore-red.log](original-gift-restore-red.log) shows an original
assertion after violating that restriction; it is excluded from compatibility
counts and is not a blocker. `probe-reference-cycle.c` is a diagnostic probe,
not a claimed valid-lifecycle gate.

These are partial drop-in results. Snapshot memory is whole-file rather than
bounded file-backed state, allocator hooks/failure injection and errno fidelity
remain unimplemented, and Windows literal-path behavior is unvalidated. Non-Unix
glob requests with actual patterns explicitly return unsupported (68). Resource
reference ownership still needs integration with filesystem capture-produced
unhashed streams, updates, overwrite, and the complete platform extraction
backends. Glob filesystem permission faults, adversarial concurrent source
changes, callback handle mutation beyond registration replacement, and all
platform metadata/security fidelity remain outstanding gates.
