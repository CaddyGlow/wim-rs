# Native C information exports

`wim` builds `libwim.so` with four real exports: `wimlib_get_version`,
`wimlib_get_version_string`, `wimlib_get_compression_type_string`, and
`wimlib_get_error_string`. Strings use static immutable NUL-terminated storage.
The baseline is original wimlib 1.14.5, not the workspace package version.
The test-support feature includes the original private test error 200; default
builds return Unknown error for that value. Unused public values also return
Unknown error. These functions need no allocation, initialization, or handles.

`probe-info-api.c` compiles against the unmodified original header and links
separately to original/native libraries. Both runs match exactly for version
and all error integers -2..202 plus INT_MIN/INT_MAX, and compression integers
-2..5. Logs and the four-symbol dynamic export listing are preserved here.
`contract-red.log` records the actual initial version failure. Rust tests check
static lifetime, numeric version fields, invalid values and both feature modes.

Reproduce on the validated Linux host:

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim --features test-support
python3 scripts/wimlib/check-info-abi.py --native target/debug
```

The original oracle was built with test support, so the candidate comparison
uses the matching feature. Default-mode behavior is separately tested in Rust.
Windows uses UTF-16 static storage in the source, but its ABI has not been
compiled or exercised. Shared-library SONAME/versioning, installation, symbol
versioning and packaging remain pending. Later codec handle exports have
separate evidence and remain partial in the API ledger. These four verified
functions are only an initial
part of the required 72-symbol replacement.
