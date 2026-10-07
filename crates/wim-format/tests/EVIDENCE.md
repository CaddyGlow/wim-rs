# Initial native header slice

Authoritative rules: upstream `/tmp/wimlib/src/header.c` read_wim_header and
write_wim_header; `/tmp/wimlib/include/wimlib/header.h` disk layout;
`include/wimlib/resource.h` resource layout; `src/wim.c` open_wim compression
selection and wim_chunk_size_valid. Fixture provenance is recorded beside it.

TDD sequence in this session: integration contract tests were written with an
empty library. Direct rustc test compilation failed with unresolved Header,
ResourceHeader, ParseError and Compression imports (E0432). Cargo initially
could not load the workspace because the parallel ms-compress crate had not yet
created its manifest. Implemented the API, then compiled both native libraries
and the test with rustc: eight tests passed. rustfmt completed; git diff --check
passed. This is a red compilation contract followed by green behavior tests,
not an assertion-failure red run. Cargo checks are recorded separately by the
workspace validation run.

Includes all 208 truncated header lengths, explicit upstream field error
codes/order, fixture known field values and exact header bytes, uncompressed
resource table size guards, zero image count permissiveness, pipable signature
and unknown flags acceptance, resource 56-bit layout, algorithm flag precedence,
chunk checks, and preserved versus canonical reserved bytes.

Not covered: reading from file descriptors or seeking to final pipable headers,
metadata/blob parsing, integrity verification, chunk tables, compression,
platform filesystem behavior, or C ABI. parse() deliberately does not normalize
boot index: upstream does so later in open_wim. Resource flags are preserved;
resource semantic validation belongs to the consuming layer.

## Built C oracle differential (2026-10-03)

`cargo test --manifest-path Cargo.toml -p wim-format
--target-dir target` passed all eight tests. Crate Clippy with
all targets/features, locked dependencies and warnings denied passed.

Built the `header_status` example then ran:

```
python3 scripts/wimlib/check-header-differential.py \
  --oracle /tmp/wimlib-native-oracle/.libs/libwim.so \
  --native target/debug/examples/header_status \
  --fixture /tmp/wimlib/tests/wims/empty_dacl.wim
```

All 20 cases matched the C public wimlib_open_wim return status: valid original
baseline; invalid magic, header size, version, three part-number conditions,
image count, three oversized tables, missing compression algorithm, nonzero
uncompressed chunk size, three codec chunk-size failures, and four truncated
headers. Mutations use full original WIM bytes except deliberate truncations;
C handles are freed and temporary files are discarded. The baseline passes
C's full open path, but the Rust probe only validates fixed headers. These
results do not establish successful Rust metadata parsing or full open_wim.
