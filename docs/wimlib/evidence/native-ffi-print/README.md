# Native printing API comparisons

The unchanged wimlib 1.14.5 header is used to compile the same C caller against
the original library and the native Rust library. Both `wimlib_print_header`
and `wimlib_print_available_images` now have native Unix implementations.

`differential.json` records 137 cases with byte-for-byte equal stdout. The
matrix includes new and pending images, ordinary, solid and pipable archives,
six image selectors, architecture names, optional and empty XML elements,
language ordering, duplicate timestamp components, numeric overflow and signed
inputs, and raw Unix filename/WTF-8 bytes. GUIDs and pending timestamps are set
explicitly through the public API to make the observations reproducible.

The original captures use `WIMLIB_DISABLE_CPU_FEATURES=sse4.2`, as recorded in
the result; this does not establish parity with the original optimized reader.
The original source baseline is commit
`cd5e231c348c255ae5088873b5a66ee0eb96fa07`.

`contract-red.log` preserves the initial missing-export linker failure.
`xml-view-red.log` and `xml-view-green.log` preserve the regression for XML
element presence and ordered children, including elements with no text.

Reproduce after building the native workspace into its explicit target directory:

```sh
python3 scripts/wimlib/check-print-api.py --native target/debug
```

These exports remain partial. Windows wide stdio, allocation failure, failing
stdout and non-default locale behavior have not been established. C numeric
conversion, UTC conversion and stdout output use the host libc; no original
wimlib code is linked into the native implementation.
