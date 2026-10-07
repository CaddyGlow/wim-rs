# Native decompressor C ABI candidate

The native shared library exports `wimlib_create_decompressor`,
`wimlib_decompress`, and `wimlib_free_decompressor`, compiled against the unchanged
`/tmp/wimlib/include/wimlib.h` baseline 1.14.5. Production code links only native
Rust decoders. The handle uses a checked, fallible Rust allocation, is opaque,
leaves caller output unchanged on failed creation, and accepts null when freed.
Invalid codec takes precedence over null output. Oversized decode returns -2
before inspecting either buffer, including null input/output buffers.

`initial-red.log` preserves the missing-export link failure of the prior shared
library. `differential.json` records 36 original-compressor blocks (three codecs,
four output sizes, three patterns). Each unchanged-header client additionally
checks 66 factory/null-output cases, output sentinel preservation, null freeing,
oversized requests, failed empty-input decoding, and successful decoding again
using the same handle. Successful bytes are checked by independent FNV-1a over
the original known plaintext as well as equality with original client output.
Malformed output contents are deliberately unspecified and not compared.
Two Rust regression tests check factory precedence/failure storage and the
oversized request accepting inaccessible buffers without dereferencing them.
A third FFI regression injects failure at every workspace/handle allocation,
requiring NOMEM (39), unchanged caller output, and recoverable cleanup.

Reproduction:

```sh
cargo build --manifest-path Cargo.toml \
  --target-dir target --locked -p wim
python3 scripts/wimlib/check-decompress-abi.py \
  --native target/debug
cargo test --manifest-path Cargo.toml \
  --target-dir target --locked -p wim --test decompress
```

## Remaining compatibility gates

This is Linux host evidence, not full API verification. The initial implementation
allocated decoder scratch per call. `allocation-red.log` records 348 allocations
across repeated decoding. The reusable LZMS workspace now owns preallocated
Huffman tables, heap/tree/sorting buffers, decision probabilities, and x86
history. Each independent block resets all adaptive state. Huffman rebuilding
uses existing capacity and in-place unstable sorting. XPRESS/LZX already use
stack scratch. The standalone LZMS function retains its fallible one-shot API.

`allocation-and-reuse-green.log` verifies zero heap allocations while repeatedly
decoding output lengths 1, 257, and 8192, malformed and successful blocks, and
random literal data that rebuilds adaptive Huffman codes. The same test injects
failure at every LZMS factory workspace allocation. The original LZMS corpus
now also decodes every record through one reusable handle, including varying
output lengths, invalid inputs, and recovery after failure.
`factory-oom-green.log` separately injects failure through the public C factory
at every workspace and final opaque-handle allocation and requires error39,
unchanged output-pointer storage, and no abort. Thus this tested allocation
schedule satisfies the no-transient-allocation decode requirement.

Original global initialization and registered C allocation callbacks are not
implemented. Platform ABI validation on Windows, concurrency stress across
independent handles, exhaustive malformed-input parity, and sanitizer gates
remain outstanding. Caller pointer validity, exclusive handle use, and disjoint
input/output buffers are required. These exports remain explicitly incomplete in public API status until the
additional requirements are verified.
