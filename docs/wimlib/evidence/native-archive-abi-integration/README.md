# Archive C ABI integration milestone

This records the historical 234-test milestone. The later
[mutable integration milestone](../native-mutable-abi-integration/README.md)
adds owned empty images and C writers, and closes the Linux arbitrary-byte
property gap described below.

The native library now exports 31 required symbols: four host-verified
information functions and 27 partial codec/archive functions. The remaining
41 public functions are unimplemented. `export-audit.json` verifies the ledger
against actual Linux ELF exports without implying behavioral completeness.

`workspace-tests.log` records 234 passing tests; `clippy.log` records strict
all-target/all-feature Clippy. Formatting, whitespace and the API documentation
inventory checks also pass. The original information probe still matches all
216 output lines when built with the matching test-support feature.

New evidence is split by owned subsystem:

- [Handle creation, opening and release](../native-ffi-handles/README.md): 288
  observations across 26 real/synthetic paths, preserving failure outputs.
- [Image properties](../native-ffi-properties/README.md): 94 matching C client
  lines; also demonstrates the unresolved arbitrary-byte Linux setter gap.
- [Header information and output settings](../native-ffi-wim-info/README.md):
  608 mutation cases, 672 setter results and five file-backed queries.
- [Lookup callbacks](../native-ffi-lookup/README.md): 11 codec/layout cases,
  exact resource fields, metadata ordering, cancellation and zero traversal
  allocations after retaining the parsed table in the handle.

Opening currently buffers the full input. Properties require Unicode text;
original Linux setters accept arbitrary bytes. Output settings need integration
with future C write operations. Newly added images and referenced external
resources need owned metadata/blob descriptors. Global initialization,
registered allocator callbacks, OOM/errno fidelity, Windows execution,
filesystem capture/apply/mounting and shared-library deployment remain open
requirements. These limitations keep all 19 newly exposed functions partial.
