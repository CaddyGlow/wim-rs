# Native byte-buffer split and join evidence

`wim-format::split_join::split_archive` partitions metadata and ordinary content
resources into a real spanned set. All metadata is in part one; content is not
repeated across parts. The split GUID is supplied explicitly by the caller (the
public file API will generate it). Ordinary encoded resources are copied without
recompression. Pipable resources are decoded and re-encoded as ordinary resources.
Solid source resources are rejected, matching wimlib's documented split limit.

The target controls stored resource payload bytes rather than final file bytes.
Metadata contributes to the first-part budget and cannot be moved. Adding a blob
starts another part at `>= target` unless the current payload size is zero. An
oversized individual resource remains indivisible. Headers, XML, lookup tables and
integrity tables add overhead beyond the target. These rules follow upstream
`src/split.c:add_blob_to_swm` at cd5e231c348c255ae5088873b5a66ee0eb96fa07.
Parts have the SPANNED header bit and correct local lookup part numbers. Multiple
parts clear boot selection as upstream `write.c` does.

`join_archives` sorts parts and verifies matching GUIDs, complete counts and unique
consecutive numbers, following upstream `src/join.c:verify_swm_set`. It resolves
content across parts, checks every used content digest, retains the GUID and XML,
and emits an unsplit ordinary WIM using native compression and optional integrity.
Missing metadata-referenced content is an error. Operations buffer their inputs
and outputs; file naming, progress cancellation, transactions and C ABI exports
are not provided by these functions.

## Verified scope

`rust-tests.log`: five contract tests cover reversed order; metadata only in first
part; retained GUID; zero target; solid rejection; missing/duplicate/foreign sets;
encoded resource accounting at exact boundaries; local lookup part numbers;
SPANNED flags and boot selection.

`differential.json`: eight bidirectional cases from original wimlib 1.14.5:
None/XPRESS/LZX/LZMS, each ordinary and pipable. Each captures four distinct files
with the original, splits to five parts with Rust, joins reversed parts with the
original, applies and compares SHA-256 content. Each also splits with the original,
joins reversed original parts with Rust, verifies with the original and applies
with content comparison. Twenty-four original invalid-set observations confirm
error 62 for missing, duplicate and foreign-GUID sets.

Reproduce after building the example:

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim-format --example split_join
python3 scripts/wimlib/check-split-join-differential.py --output docs/wimlib/evidence/native-split-join/differential.json
```

Evidence does not establish all public write flags, callbacks, split filename
rules, bounded-memory streaming, allocation-failure parity or platform gates.
