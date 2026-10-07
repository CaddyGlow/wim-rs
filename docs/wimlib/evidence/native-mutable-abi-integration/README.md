# Mutable handle and C writer integration milestone

The candidate now exports 36 of 72 required symbols: four host-verified
information functions, 32 partial functions, and 36 unimplemented functions.
`export-audit.json` checks actual symbol presence against this ledger.
`workspace-tests.log` records 244 passing tests; strict Clippy is preserved in
`clippy.log`. Formatting and documentation inventory checks also pass.

This integration connects owned images, edited properties, output settings,
lookup callbacks and verification to actual ordinary, solid and pipable output:

- [Image mutation](../native-ffi-image-mutation/README.md): 4,932 observations
  across 13 cases; real pending metadata, deletion, boot/XML consistency and
  preservation of file-backed zero-reference descriptors.
- [Properties](../native-ffi-properties/README.md): 1,498 observations and 15
  write outcomes; actual owned byte XML closes arbitrary-byte Linux getter and
  setter compatibility, including failure and correction before serialization.
- [C writing](../native-ffi-write/README.md): 240 cases using paths/descriptors,
  current/added/deleted images, all ordinary codecs and three output layouts.
  Original verification validates 480 outputs; extraction checks 240 native
  images. Separate ordinary/solid
  settings are consumed; source hashes remain unchanged.
- [C verification](../native-ffi-verify/README.md): 17 valid/corrupt archive cases,
  including verification after deletion and true owned empty-image metadata.

The original optimized SSE4.2 LZMS reader crashes on a 48-byte payload in both
original and native output. The writer matrix uses the original supported
CPU-feature-disable configuration; default-path crash inputs and the GDB trace
are preserved in the writer evidence. Default optimized-reader compatibility
must not be inferred from the generic reader's passing results.

Full drop-in parity is still incomplete. File/resource/output buffering,
compression-level tuning, allocator and lifecycle contracts, callbacks,
overwrite and reference contexts, remaining write flags, failure timing,
Windows text/ABI/backends and filesystem capture/apply/mounting remain gates.
The full implementation plan retains these requirements without reducing scope.
