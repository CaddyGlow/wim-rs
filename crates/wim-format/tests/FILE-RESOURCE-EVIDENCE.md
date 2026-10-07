# File-backed resource range evidence

`file_resource::read_resource_range` accepts a `Read + Seek` source and reads
ordinary, pipable, or solid resource ranges. It does not load the file or whole
resource. Output allocation is the selected range; compressed input and decoded
scratch are each at most one resource chunk, excluding caller codec workspace.
Ordinary tables use only selected boundary entries. Solid tables require a prefix
sum; their entries are streamed through an eight-byte stack buffer. The latter
bounds memory but still requires O(first selected chunk) table I/O and seeks.

The framing algorithm follows `/tmp/wimlib/src/resource.c` from wimlib 1.14.5
commit `cd5e231c348c255ae5088873b5a66ee0eb96fa07`. This is an internal Rust primitive,
not yet `wimlib_open_wim`, a pipe reader, or a hash-verifying extraction operation.
Partial range reads cannot establish the complete blob SHA-1.

Eight tests include original ordinary/pipable/solid fixtures and sparse sources:
a 1 TiB raw file reads ten selected bytes; a 1 TiB compressed-framed ordinary
resource reads sixteen boundary bytes plus ten raw chunk bytes; a solid table
reads four-byte prefix entries without buffering the table; a compressed range
reads one intersecting body and invokes its decoder once. Invalid selected
boundaries, oversized truncated file spans, and original OS errors are covered.
Source inspection, together with these observed I/O tests, supports the allocation
bound. The mock tests do not measure process RSS or prove real filesystem latency.

The `read_file_range` example additionally opens headers and the complete lookup
resource, then calls this reader for the requested blob range. Its lookup table
allocation is deliberately outside the resource reader's bound. It never loads
the complete WIM. 176 comparisons against original `read_partial_wim_blob_into_buf`
passed across None/XPRESS/LZX/LZMS, ordinary/pipable/solid, two blob sizes, empty
ranges, partial final chunks, and crossing boundaries. Reproduce with:

```
cc -I/tmp/wimlib-native-oracle/include -I/tmp/wimlib-native-oracle \
  scripts/wimlib/read-range-oracle.c /tmp/wimlib-native-oracle/.libs/libwim.a \
  -lpthread -lm -o /tmp/file-read-range-oracle
cargo build --manifest-path Cargo.toml \
  --target-dir target --locked -p wim-format --example read_file_range
python3 scripts/wimlib/check-range-differential.py \
  --capture /tmp/wimlib-native-oracle/wimlib-imagex \
  --original /tmp/file-read-range-oracle \
  --native target/debug/examples/read_file_range
```

Logs and comparison hashes are in `docs/wimlib/evidence/native-file-reading/`.
The initial red log is an unresolved-module test run before implementation/export;
it is a compile-time missing-feature failure, not a behavior assertion failure.
