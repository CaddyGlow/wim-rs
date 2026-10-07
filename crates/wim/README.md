# WIM engine and Rust API

This crate implements the roadmap in
[the implementation plan](https://github.com/CaddyGlow/wim-rs/blob/main/docs/wimlib-rust-implementation-plan.md).
The `windows-uup` package uses the `wim` crate in process through its safe Rust
API. The compatible C exports also support independent ABI validation; this
implementation is not yet a complete drop-in replacement for upstream wimlib.

```sh
cargo test --locked -p wim -p wim-format -p wim-types --all-features
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

The root workspace includes `crates/wim`, `crates/wim-format`,
and `crates/wim-types`. WIM licensing is explicit in these
manifests and in the workspace defaults.

`src/api.rs` owns the public Rust interface and forbids unsafe code. It owns a
boxed engine handle and calls typed Rust operations directly; it does not call
the exported `wimlib_*` functions or marshal requests through `wim::ffi`.
The C adapters validate borrowed pointers and delegate to the same shared engine
operations. Native cancellation closures run synchronously through a scoped Rust
registration; C callers retain their separate C progress callback contract. `src/engine/` contains low-level
archive, capture, extraction, writing, and platform implementation modules.
`src/ffi.rs` explicitly exports the compatible C entry points, layouts, and
callbacks. Rust applications use `wim::Wim`; raw Rust callers of the C API
migrate from `wim::wimlib_*` to `wim::ffi::wimlib_*`. Engine validation uses the
hidden `wim::engine` namespace, which is not the application API.
The safe `Info` value returns a `Compression` enum and excludes C reserved
storage and bitfields.

The `wim-types` crate defines shared format constants; `wim-format` parses and
serializes image data. Ownership uses `Vec`, `Box`, `Arc`, and hashbrown
collections with Rust's global allocator. The portable format core supports
`no_std + alloc` with default features disabled.
The engine crate was formerly named `wim-ffi`. It now exposes an owned `Wim`
archive, validated `ImageIndex`, `Info`, `OpenOptions`, `Compression`, and typed `Error`
values. Rust callers use this API; handle cleanup, platform string encoding,
and numeric error conversion are internal. `Wim::overwrite` consumes the archive,
so callers must reopen it after committing. The `libwim` artifact names are retained. Custom allocator registration is
deliberately removed from the C interface. Builds use the root `target/`.
Historical evidence logs retain the package name recorded when they were generated.

```rust
use wim::{OpenOptions, Wim};
use std::path::Path;

let mut archive = Wim::open(Path::new("install.wim"), OpenOptions::default())?;
archive.verify()?;
```

Compression is provided by the published crate
[`ms-compress`](https://crates.io/crates/ms-compress), which uses Rust's global allocator for
codec scratch storage and WIM container ownership. Checked buffer and hash-table
growth uses fallible reservation and reports NOMEM where supported. Ordinary
`Box`/`Arc` allocations follow Rust's global allocator failure policy; complete
recoverable out-of-memory behavior is not promised.

`wimlib_set_memory_allocator` and its callback types are removed. C-owned XML
and text results remain allocated by the fixed host C runtime and must be
released with the matching runtime's `free`. Opaque handles use Rust-owned storage; codec contexts retain their private
allocation strategy. Both must be released through their corresponding `wimlib_free*` functions.
Rust-owned containers must never be passed to C `free`.

The API ledger records selected compatible exports and their remaining behavior
gates. Custom allocator registration is explicitly marked removed. Historical
export counts and allocator-hook evidence describe earlier artifacts, not the
current interface. Windows runtime validation, level tuning, memory budgeting,
lifecycle, and platform gates remain separate. The
[Windows cross-build evidence](../../docs/wimlib/evidence/native-windows-cross-build/README.md)
records earlier MSVC and GNU compilation/linkage.
The [Windows named-stream capture evidence](../../docs/wimlib/evidence/windows-capture-ads-20261005/README.md)
records file, directory, root, empty, Unicode, and hard-link ADS capture followed
by native and independent DISM apply. The safe capture API also exposes a strict
security policy. This selected gate does not establish full metadata parity or
native offline NTFS/QCOW2 capture.

The [expanded Windows metadata evidence](../../docs/wimlib/evidence/windows-capture-full-20261005/README.md)
covers no-follow stream enumeration and deferred backup reads, including protected
files, symlink/junction ADS, large stream lists, ACL/SACL and integrity labels,
sparse/compressed files, short names, EFS, extended attributes, and object IDs.
The retained comparisons distinguish exact upstream/Rust metadata equality from
documented DISM differences. Whole-system installation and native offline NTFS
capture remain separate gates.
Captured reparse directories with named ADS use a zero main hash and extra
streams ordered as the reparse payload followed by named DATA, without an
unnamed directory DATA entry. Concrete and deferred binding regressions check
this layout and idempotence. The 2026-10-05 offline installation source contains
no reparse directory ADS, so its installation result does not exercise this edge.
The [Windows runtime evidence](../../docs/wimlib/evidence/native-windows-abi/README.md)
compares 278 behavior/layout rows and 65 exports with the original DLL using the
same MinGW caller. Full Windows filesystem and NTFS behavior remain unverified.
The [original portable suites](../../docs/wimlib/evidence/native-full-upstream/README.md)
pass with the original CLI linked to native, including the strengthened comparator.
The [local Linux package](../../docs/wimlib/evidence/native-linux-packaging/README.md)
has the original `libwim.so.15` SONAME and audited header/artifact hashes.
No production package links to the upstream C library. Reference C builds are
test-only oracles. Keep upstream source and inputs unchanged; build an isolated
copy using `scripts/wimlib/build-oracle.sh`.

Source-derived translations use upstream-compatible licensing, declared in
the workspace manifest. Their individual provenance records identify original
files. This does not change the license of the existing windows-uup package.

Compare the safe Rust API and C interface on Linux with matched capture, write,
open, verify, and extraction workloads:

```sh
python3 scripts/wimlib/benchmark-apis.py --output /tmp/wim-api-results.json
```

The driver builds one release executable containing both call paths, warms up
each codec, alternates API order, and records individual samples and medians for
time, peak process RSS, and archive size. Both APIs read the same reference archive;
every written archive is checked through the other API against source hashes.
Use `--oracle /path/to/original/wimlib-imagex` for independent verification.
The four ordinary codecs use identical defaults; solid compression is covered
separately by the existing C-interface `benchmark-performance.py` benchmark.

[Windows C-runtime printing](../../docs/wimlib/evidence/native-windows-print/README.md)
matches both original text-mode and binary-mode raw output using the same
MinGW/MSVCRT caller. This selected result does not establish all locale and CRT
variants.

Applications can use `wim = "0.1.2"` from crates.io. Enable the `disk-capture` feature for immutable offline NTFS capture. This crate and its tests build without sibling repositories. WIM fuzzing is independently owned by this repository’s `fuzz` package.
