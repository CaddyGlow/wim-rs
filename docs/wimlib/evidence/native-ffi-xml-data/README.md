# Native original-XML C API candidates

`wimlib_get_xml_data` returns a C-allocated copy of the original XML resource,
including its exact UTF-16LE bytes. Editing image properties does not replace
that original resource. The caller releases the copy with the matching host C
`free`. `wimlib_extract_xml_data` appends those bytes to the caller's C `FILE`,
leaves it open, and reports write failure as error 72.

`contract-red.log` preserves the missing-export test failure. The corrected
`contract-green.log` covers new handles and null output validation. An unchanged
public-header C client compares exact getter and stdio results in 11 archives:
ordinary and pipable output with all four codecs, and solid output with three
compressed codecs. `differential.json` records the matching observations and
output digests. Tests also check property edits, stdio append position, and a
read-only stream's failure result. Contrary to a possible interpretation of the
NO_FILENAME documentation, the original library succeeds on a newly created
handle with a zero-byte resource; the native implementation matches that result.

Reproduce on Linux after building the native library:

```sh
cargo test --manifest-path Cargo.toml --target-dir target --locked -p wim --test xml_data
python3 scripts/wimlib/check-xml-data-abi.py --native target/debug
```

These two exports remain partial. Registered allocator callbacks, Windows CRT
ownership, file-descriptor/stream-only handles, bounded-memory access, allocation
failure injection and complete malformed-resource failure timing remain gates.
Only host C allocation and stdio functions are called; no production C wimlib is
linked into the native implementation.
