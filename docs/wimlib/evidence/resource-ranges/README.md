# Native resource range evidence

The resource reader supports checked uncompressed ranges in raw, ordinary,
seekable pipable and solid resources. It decodes only intersecting chunks.
Partial boundary chunks use one reader scratch buffer at a time; full chunks
decode directly into the requested output. Codec workspace is additional. The
input WIM is still borrowed as a complete byte slice, so this does not yet
establish bounded file-backed archive I/O.

`contract-red.log` records two failing stub tests; both contracts now pass.
They check raw bounds, no raw decoder calls, leading/trailing/empty ranges and
chunk-crossing data with at most two codec calls. `differential.json` records
176 exact-output comparisons against original `read_partial_wim_blob_into_buf`.
The oracle links the original static build with private original headers,
solely for testing. Eleven codec/layout combinations each contain two distinct
blobs, exercising solid blob-relative offsets, complete/empty reads, and ranges
crossing chunk boundaries. Results also match independently known input bytes.

`workspace-tests.log` records 189 passing tests after adding the solid/pipable
writers, new-image builder, and resource range reader. Strict all-target and
all-feature Clippy passes. Archive range reads deliberately do not perform
whole-blob SHA-1 verification; complete reads retain that check.

Reproduce:

```sh
cc -I/tmp/wimlib/include scripts/wimlib/read-range-oracle.c /tmp/wimlib-native-oracle/.libs/libwim.a -lpthread -o /tmp/wim-read-range-oracle
cargo build --manifest-path Cargo.toml --target-dir target --locked --example read_range
python3 scripts/wimlib/check-range-differential.py --capture /tmp/wimlib-native-oracle/wimlib-imagex --original /tmp/wim-read-range-oracle --native target/debug/examples/read_range
```

Malformed-range error parity against the private C reader is not claimed: it
assumes its caller has already validated ranges. Native readers validate them.
Compressed framing is preflighted across the complete resource; differential
failure behavior for truncation outside a selected range remains a separate
gate. Cache reuse, callbacks/cancellation and descriptor-backed I/O remain.
