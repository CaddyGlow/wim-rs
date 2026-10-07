# Native codec context evidence

The `wim-codec::context` module provides native, safe codec handles and explicit
configuration resolution, grounded in wimlib commit
`cd5e231c348c255ae5088873b5a66ee0eb96fa07` (`src/compress.c`,
`src/decompress.c`, and each codec's creation function).

`Decompressor` accepts numeric XPRESS/LZX/LZMS types, rejects uncompressed or
unknown types, enforces codec-specific maximums (65536, 2097152, 1073741824),
and distinguishes an output-size rejection from malformed compressed data.
Handles can be used again after either failure. Rust ownership replaces the C
free function. Factory validation requires no allocation; scratch allocations
occur during individual codec calls.

`FixedStrategyCompressor` provides reusable compression handles for all three
native encoders with empty/oversized input and capacity failures returning
`None`. Input remains immutable. It intentionally exposes no compression level
argument: the current encoders do not implement upstream's level-dependent
search and optimization policies. Native roundtrip tests reuse every codec
three times, including after capacity rejection.

`CompressionDefaults::resolve` validates and resolves the original configuration
semantics independently of the fixed-strategy handle. Explicit level bits must
fit 24 bits after removing bit 31. Level zero uses a per-codec default, then
50 if that default is zero. The original setter accepts arbitrary unsigned
defaults, and resolution does **not** validate or remask these defaults; tests
preserve that unusual behavior. Defaults are owned values rather than mutable
process globals.

## Reproduction

```sh
cargo test --manifest-path Cargo.toml --target-dir target --locked -p wim-codec --test context
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim-codec --example context_probe
python3 scripts/wimlib/check-context-differential.py --output docs/wimlib/evidence/codec-contexts/factory-differential.json
```

Four Rust integration tests passed. `factory-differential.json` records 92
comparisons with real original compressor/decompressor creation calls covering
type errors, zero/oversized maximums, valid small maximums, level limits, and
destructive flags. Another 18 maximum-boundary cases compare native validation
with original decoder creation and nonzero original compressor memory queries;
large original compressor allocations are deliberately not attempted.

This proves configuration behavior and handle reuse, **not** a complete C API
replacement. Null output pointers, global initialization, configurable allocator
hooks, default-table global synchronization, scratch allocation reuse,
compression level tuning, destructive in-place processing, exact memory-query
values, and C ABI exports remain separate gates. No red-before-green log was
captured for this package; these tests are regression evidence.
