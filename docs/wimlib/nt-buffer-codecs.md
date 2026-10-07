# Windows buffer codecs

`ms-compress::lznt1::{compress, decompress}` implements LZNT1 (Windows format
2). `ms-compress::xpress_plain::{compress, decompress}` implements plain XPRESS
(Windows format 3). Both are pure Rust and covered by the crate's
`forbid(unsafe_code)` policy. Existing `decompress_xpress` and
`xpress_encode::compress_xpress` remain the WIM XPRESS Huffman APIs.

Compression returns an owned buffer and uses a bounded greedy match search.
Allocation is fallible, and output can exceed input size. Decompression accepts
a caller-owned output slice, performs no allocations, and returns bytes written.
Errors can leave partial output. These are buffer codec APIs, not Windows ABI or
NTSTATUS replacements; compression engine levels and Windows parse choices are
not reproduced.

The implementation follows [Microsoft MS-XCA](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-xca/).
LZNT1 resets history every 4096-byte chunk and stores incompressible chunks raw.
Plain XPRESS uses an 8192-byte history and supports shared length nibbles and
16/32-bit extended lengths, including across flag groups. Its encoder emits a
terminal flag group; its decoder requires the terminal match flag at input end.
LZNT1 accepts an optional zero terminal header followed by zero padding.

Host regression coverage includes published MS-XCA sections 3.1 and 3.3 vectors,
round trips over repetitive and deterministic random inputs, output bounds,
chunk and flag boundaries, extended lengths, malformed offsets, truncations,
and all two-byte inputs. Host tests do not establish native Windows compatibility.

Run host checks:

```sh
cargo test -p ms-compress --locked
cargo clippy -p ms-compress --all-targets --all-features --locked -- -D warnings
```

The native gate calls ntdll in both directions for both formats, using random,
repetitive, and patterned input at format boundaries. It must be run on Windows:

```sh
cargo test -p ms-compress --locked --test nt_buffers_windows -- --ignored
```

Native Windows validation has not yet been performed for these APIs.
