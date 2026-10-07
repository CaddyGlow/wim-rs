# Native compressor C ABI candidate

The native library now exports `wimlib_create_compressor`, `wimlib_compress`,
`wimlib_free_compressor` and `wimlib_set_default_compression_level`. These are
partial implementations, not fully verified drop-in exports.

`contract-red.log` records the integration tests failing before these exports
existed. Two Rust tests now cover opaque handle ownership, output pointer
preservation on factory error, reuse after capacity/maximum failures, null free,
and all three codec round trips. The added `FixedStrategyCompressor::maximum`
accessor permits the C boundary to reject oversized requests before constructing
an input slice.

The unchanged `/tmp/wimlib/include/wimlib.h` compiles
`scripts/wimlib/probe-compress-api.c` with warnings denied. It dynamically loads
the native candidate and original oracle independently. `interoperability.json`
records 30 native compressor/original decompressor cases across three codecs,
two inputs and five level/flag requests, plus 120 factory return-code and failure
output-pointer comparisons. Each compressor also rejects empty, oversized and
zero-capacity requests, then succeeds again. Destructive permission preserves
input, which the original contract allows. Null free and default setter error
contracts are checked.

Reproduce after building `wim`:

```sh
cc -Wall -Wextra -Werror -I /tmp/wimlib/include scripts/wimlib/probe-compress-api.c -ldl -o /tmp/wim-probe-compress
/tmp/wim-probe-compress target/debug/libwim.so /tmp/wimlib-native-oracle/.libs/libwim.so
```

Remaining gates: encoders currently use their existing fixed search strategy;
accepted levels do not select the original compression tuning policies. Codec
scratch is allocated per call, not eagerly at creation. Custom allocator hooks,
original global initialization/error propagation, allocation failure injection,
Windows ABI execution, representative ratio/performance benchmarking and full
codec fuzzing are unverified. The opaque handle allocation itself is fallible;
codec allocation failure is converted to zero by the compression call. This
candidate does not claim the original peak-memory or factory allocation timing.
No original C code is linked into the production native library.
