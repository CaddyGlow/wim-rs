# Native XPRESS encoding evidence

The native encoder in `wim-codec/src/xpress_encode.rs` uses a bounded 64-candidate
hash-chain greedy LZ77 parser and frequency-built canonical Huffman tree. The
emitter handles overlapping matches, 16-bit offset ranges, short and extended
match lengths, interleaved 16-bit little-endian coding words, and Microsoft's
end-of-data symbol. If an unconstrained Huffman tree exceeds the format's
15-bit limit, a complete balanced tree over the used symbols provides a valid
bounded fallback. No original C code is linked by the production crate.

The format reference is `/tmp/wimlib`, revision
`cd5e231c348c255ae5088873b5a66ee0eb96fa07` (`src/xpress_compress.c` and
`src/xpress_decompress.c`), LGPL-2.1-or-later. The native implementation carries
that license notice. Tests use the separately built original library at
`/tmp/wimlib-native-oracle/.libs/libwim.so` only as an independent oracle.

`contract-red.log` records the actual initial stub run: four of six behavioral
contracts failed. `rust-tests.log` records the completed encoder contracts and
existing decoder suite. Encoder coverage includes block-size errors, small
input and insufficient capacity, original two-byte trailing capacity requirements, overlap, lengths
through a 65536-byte block, periodic binary bytes, generated low-entropy input,
expanded incompressible data with generous capacity, and bounded complete Huffman fallback.

`oracle-roundtrip.json` records 706 generated input/capacity cases: 353 inputs
with both tight capacity and generous capacity. Inputs include periodic random
patterns with distances through 32768, match-length boundaries, 256 low-entropy
distributions, 64 random-byte distributions, and size edges. 632 calls returned
compressed blocks; all 632 were accepted by the original C XPRESS decoder and
produced byte-for-byte identical input. 67 of these expanded their input with
sufficient capacity. The Rust example also verifies the native decoder before
writing every compressed block. The 74 non-output cases reflect native
capacity/small-input outcomes; parse choices and sizes can differ from original.
Original compressor sizes for the same input/capacity are recorded separately.

The original C compressor was additionally probed with 4000 constant bytes:
its output is 263 bytes, capacities 262/263/264 return zero, and capacities
265/266 return 263. The native helper now requires the same two unused trailing
capacity bytes beyond its own output length. `capacity_observations` preserves
these live oracle results. `expansion-contract-red.log` and
`capacity-contract-red.log` preserve failing regression runs before both fixes.
The earlier 353-case run is retained as
`oracle-roundtrip-before-expansion-fix.json`; it documents historical behavior,
not the current contract.

Reproduce from the repository root:

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim-codec --example xpress_encode
python3 scripts/wimlib/check-xpress-encoder.py --output /tmp/xpress-encoder-oracle.json
cargo test --manifest-path Cargo.toml --target-dir target --locked -p wim-codec
```

This proves native compressor/decoder interoperability for the recorded cases,
not drop-in compressor API completion, production performance, or compressed
byte identity. Compression levels, optimal/lazy parsing, original allocation
hooks, and original compressor-handle lifecycle remain absent. Expanded output
is now allowed when capacity permits it, matching the low-level C API; WIM
resource callers must independently choose raw storage when output does not
shrink input. Memory storage is bounded by block size and allocated fallibly; allocator
failure has not been injected. Large-file/WIM writer integration remains a
separate gate.
