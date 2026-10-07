# Native compression memory query compatibility

`wim/src/engine/memory.rs` implements the `wimlib_get_compressor_needed_memory`
export as native Rust, translating the allocation formulas in original
`compress.c`, `xpress_compress.c`, `lzx_compress.c`, `lzms_compress.c`, and
`lcpit_matchfinder.c` at commit
`cd5e231c348c255ae5088873b5a66ee0eb96fa07` (1.14.5).
There are no runtime C dependencies. The current profile is 64-bit pointer
width; the export is deliberately absent on 32-bit builds. Only Linux x86-64
has oracle evidence. Windows structure alignment and other targets remain gates.

## Contracts and differential evidence

Four Rust contracts cover invalid requests, XPRESS level 60, LZX level 34/35
and 32768/32769 position widths, and LZMS temporary suffix-array floor and
2^26 interval-width transition. `contract-red.log` records three genuine
assertion failures with the memory function initially returning zero;
`contract-green.log` records the final implementation's passing contracts.

`probe-compression-memory-layout.c` includes each preserved original compressor
source separately and prints the exact x86-64 structure offsets/sizes. The
three `*-layout.txt` files record its outputs. The Rust formula includes the
32-byte original outer compressor, not the size of the native opaque handle.

`check-compression-memory-abi.py` loads original and native shared libraries via
ctypes with explicit C argument and result types. `differential.json` records
29,646 identical query results and 70 identical setter return values, including
invalid signed codec values, zero and maximum-sized requests, near-boundary
sizes, random requests, valid/invalid explicit levels, destructive flags,
per-codec/all-codec defaults, and arbitrary raw defaults such as UINT_MAX.
No large input buffers are allocated during queries. Default resolution uses
the compressor module's shared raw defaults. Like original C, only the explicit
level is masked/validated; a raw default is neither masked nor revalidated.

Reproduce after building original and native shared libraries:

```sh
python3 scripts/wimlib/check-compression-memory-abi.py \
  /tmp/wimlib-native-oracle/.libs/libwim.so \
  target/debug/libwim.so
```

## Remaining semantic gap

The original public contract calls the result an approximate number of bytes
needed to allocate its compressor. This implementation reproduces those
observable baseline estimates exactly. Current native fixed-strategy encoders
have different allocation lifetimes and scratch sizes, so the result is **not
yet a verified native memory budget**. Native eager workspace allocation,
allocator hooks, level-dependent strategies, and allocation-failure timing must
be implemented and reconciled before this export can claim complete drop-in
semantics. Query compatibility alone must not be marked `host_verified` in the
full API completion ledger.
