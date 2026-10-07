# Codec C ABI integration milestone

The Linux native library exports 12 of the 72 required symbols. Four information
functions remain host-verified; eight codec functions are partial. Symbol
presence does not establish behavioral or ABI parity. `export-audit.json`
checks this distinction against the public API ledger.

The codec evidence covers unchanged-header clients, native/original block
interoperability, factory validation, handle reuse, output guards, and original
memory query/default behavior. See the adjacent `native-ffi-decompress`,
`native-ffi-compress`, and `native-compression-memory` evidence directories.

`workspace-tests.log` records 222 passing workspace tests. `clippy.log`
records strict all-target/all-feature Clippy. Formatting and API documentation
inventory checks also pass. The information client must use `test-support`
when compared with the test-support-enabled original oracle; default builds
intentionally omit the original private test error 200.

Reusable LZMS contexts now allocate their scratch at factory creation. Tests
count zero allocations across repeated successful, malformed and varying-size
decodes, replay the original LZMS corpus through one reused context, and inject
failure at every factory allocation position. Factory failures return NOMEM
and leave the C caller's output pointer unchanged.

Remaining drop-in requirements include compression level tuning, registered
allocation callbacks, native compressor memory bounds and eager workspaces,
global initialization, all remaining archive/operation exports, filesystem
backends, platform ABI validation, and deployment packaging. Neither a valid
compressed block nor a matching memory query closes these gates.
