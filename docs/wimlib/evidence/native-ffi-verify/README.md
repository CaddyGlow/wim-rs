# Native verification C API

`wimlib_verify_wim` validates the current image set, including owned empty
images and source-image mappings after deletion. It parses metadata, resolves
nonzero canonical stream hashes, and checks SHA-1 for every retained data blob,
including file-backed blobs whose recorded reference count reached zero.
Metadata SHA-1 failures return 21, while file-data SHA-1 failures return 28,
matching the original library. Metadata expansion is limited before allocation
using the original 512-times-file-size guard.

The initial Rust test failed because the export was absent; it passes after
implementation. `differential.json` records 17 unchanged-header comparisons:
11 ordinary, solid and pipable codec/layout cases, plus data-hash corruption,
metadata-hash corruption, malformed security structure with a corrected digest,
a missing stream with a corrected metadata digest, out-of-file payload offsets,
and a corrupted integrity table. Verification intentionally does not check
the integrity table; that is a separate open-time option in original wimlib.
Each client also checks flags, null input, verification after deleting all
images, new empty handles, and owned empty-image verification.

Reproduce:

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim
python3 scripts/wimlib/check-verify-abi.py --native target/debug
```

This export remains partial. Metadata/resources are buffered; bounded streaming
verification, progress callbacks and cancellation, externally referenced
resources, allocator callbacks, exhaustive malformed-input parity, and platform
validation remain required. This evidence covers ordinary API outcomes, not
the full original failure and resource-use guarantees.
