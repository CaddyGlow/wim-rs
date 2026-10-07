# Native C split and join candidates

The native library now exports `wimlib_split` and `wimlib_join`. It does not
export `wimlib_join_with_progress`: callback lifecycles remain unimplemented,
so that API remains pending rather than silently ignoring the callback.

Splitting partitions stored resource bytes, keeps every retained image's metadata
in part one, writes conventional numbered filenames and creates a shared fresh
GUID unless RETAIN_GUID is requested. Ordinary encoded payload bytes are copied.
The source file is preserved. Current XML/GUID/boot/rpfix/readonly settings and
deleted original images are reflected in the split set; remaining image mappings
still identify their original metadata. Pending or externally owned images are
rejected, matching the original unchanged-image requirement for pending additions.
Reference counts use current handle lookup state, including retained zero-count
backing blobs after deletion. Headers, tables and XML lie outside the partition
size budget; an indivisible resource may exceed the target.

Joining validates a complete consecutive set with matching GUID/part counts,
accepts arbitrary order, combines verified data and image metadata, retains the
split-set GUID and writes ordinary, pipable or solid output. Solid output uses
independent ordinary metadata settings and the default LZMS solid settings.
Native opening performs requested integrity/access/split checks on every source.

`contract-red.log` records missing-export failure. `contract-green.log` records
two Rust tests for parameter rejection, filename insertion before the final
suffix, source preservation, XML edits, reverse-order joining and rejection of
missing/duplicate parts. Strict Clippy passes for the complete FFI crate.

`probe-split-join-api.c` includes the unchanged original public header and compiles
with warnings denied. `check-split-join-api.py` records 96 original/native split
comparisons: ordinary/raw, XPRESS and pipable sources; four partition budgets;
three integrity preferences; current XML/header edits and deleted/pending images.
There are 60 successful split comparisons and 36 equal pending-image rejections.
Successful sets match filenames, partition/image/boot/header fields, GUID,
reference counts, content hashes and XML. Each successful set is joined in both
original/native directions with ordinary/pipable/solid output, for 360 cross-joins
verified and applied with the independent original library. Missing and duplicate
part sets return original error 62. Input source SHA-256 remains unchanged.

Additional evidence:

- `filename-differential.json`: plain, leading-dot, multiple-dot and Unicode names.
- `invalid-parts.json`: foreign GUID returns 62; malformed metadata digest during
  join returns 28. This differs from standalone write's observed metadata error 21.
- `pipable-input-join.json`: original-produced pipable split sets join with default
  pipable output or forced ordinary/solid output, including contradictory mode
  rejection (24).

Reproduce after building `wim`:

```sh
python3 scripts/wimlib/check-split-join-api.py
```

All original oracle processes use its supported
`WIMLIB_DISABLE_CPU_FEATURES=sse4.2` configuration. The default optimized-reader
LZMS counterexample is retained in the separate native-ffi-write evidence; the
original source is unchanged.

Remaining gates: complete progress/cancellation/name-override callbacks,
allocator hooks, bounded-memory file streaming, close-error/failure timing,
Windows GUID generation, and split recompression/chunk-size changes. Split
pipable output, solid output, delta/done-with-file/solid-sort policies currently
return unsupported (68); contradictory/publicly invalid flags return 24.
Pipable input can be split into ordinary output when NOT_PIPABLE is requested,
but deletion/reordering of pipable input images remains unsupported. Joining
supports ordinary/pipable/solid modes but delta/done-with-file/solid-sort policies
remain unsupported. These are candidate partial exports, not complete drop-in
implementations. No original C code is linked into the native production library.

The unchanged randomized caller exposed split `NO_SOLID_SORT` flag0x4000 as a
real unsupported gate. Original split.c delegates an unchanged non-solid resource
plan; this flag has no sorting work there, while existing solid source resources
still return68. The native split now follows that contract. `no-solid-sort-red.json`
preserves the initial failure, and `no-solid-sort-initial.json` records the fix:
all16 control/no-sort cases match, including original join, verification,
application and payload hashes. The broader32-case matrix retains8 additional
SOLID-output request differences; these are not covered by this flag milestone.
Input archives and original source remain unchanged.

```sh
python3 scripts/wimlib/check-split-no-solid-sort.py --output docs/wimlib/evidence/native-ffi-split-join/no-solid-sort-initial.json
```

The next unchanged randomized failure was split flags0x5 (pipable with integrity).
The real lazy resource plan already serializes pipable resource headers, compressed
chunk framing, first-part-only image metadata, per-part XML and integrity; the
obsolete public split gate now permits that actual path. `pipable-valid-red.json`
preserves48 cases/32 complete matches before the fix; `pipable-initial.json`
records48/40 with all8 newly supported pipable cases equal, including independently
joined, verified and applied output. The remaining8 are SOLID output requests.
`pipable-red.json` preserves an intermediate harness error: combining PIPABLE4
with forced NOT_PIPABLE8 only tested contradictory flag rejection. The corrected
runner retains NOT_PIPABLE only for ordinary variants; no result normalization
was added. Fresh fixed native SHA is
`a09caebe095efca042a4b66d11f7b1dffd07422d1d3a69fd13903a4973939055`.
