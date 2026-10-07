# Native resource serialization evidence

`wim-format::resource_write::encode_resource` implements whole-resource framing
from upstream `src/write.c` at `cd5e231c348c255ae5088873b5a66ee0eb96fa07`.
It emits ordinary leading cumulative offsets, pipable size-prefixed chunks with
trailing cumulative offsets (excluding the size prefixes), and solid alternate
headers with a table containing every compressed chunk size. Nonshrinking or
empty compressor results store the chunk raw. Allocations use `try_reserve`;
length arithmetic is checked and descriptors reject sizes outside 56 bits.

Nine Rust contract tests cover partial and exact chunk boundaries, framing bytes,
raw fallback, empty and uncompressed input, invalid chunk sizes, and compressor
error propagation. They initially failed to compile with the absent exported
module; following implementation and export all pass (`rust-tests.log`). This
is API compile-red evidence, not a preimplementation runtime-red test.

`c-reader-differential.json` records 18 independently extracted resources:
XPRESS/LZX/LZMS × ordinary/pipable/solid × mixed compressed/raw or all raw.
The 65,554-byte deterministic payload includes a repeated-byte first chunk,
pseudorandom incompressible middle chunk, and partial final chunk. The original
C compressor generates chunk inputs only. Rust frames them into resources.
The script relocates resources and lookup descriptors in C-captured archives;
original C extraction must reproduce the complete payload byte for byte.
Production Rust resource serialization has no C dependency.

Reproduce after building `wim-format --example encode_resource`:

```sh
python3 scripts/wimlib/check-resource-write-differential.py \
  --oracle /tmp/wimlib-native-oracle/wimlib-imagex \
  --library /tmp/wimlib-native-oracle/.libs/libwim.so \
  --native target/debug/examples/encode_resource
```

This does not establish complete archive writing, pipe streaming, pipable blob
headers, native compressor behavior, or large-resource allocation feasibility.
The 64-bit ordinary offset-table branch for resources exceeding 4 GiB is
implemented from the source rule but has not been exercised by this evidence.
