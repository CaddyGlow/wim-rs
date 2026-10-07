# wim-rs

Reusable Rust WIM implementation, extracted from windows-uup.

- `crates/wim`: native WIM API and C ABI, capture, extraction and writing.
- `crates/wim-format`: WIM parsing and serialization, with optional `std`.
- `crates/wim-types`: shared WIM types, with optional `std`.
- `scripts/wimlib`: differential probes and validation tools.
- `docs/wimlib`: retained validation evidence. Historical receipts are unchanged.
- `fuzz`: bounded WIM parser harness and honggfuzz executable.

Keep this checkout beside `../windows-uup` and `../ms-compress`. Compression
comes from the standalone ms-compress repository. Optional `windows-disk` and
`windows-ntfs`, and the patched NTFS dependency still live in windows-uup. This extraction does not duplicate those libraries. UUP servicing and
archive adapters remain in windows-uup and consume this workspace by path.

Run from this directory (use `nix develop` for the pinned toolchain):

```sh
cargo build --locked
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test -p wim-types -p wim-format --no-default-features --locked
cargo check -p wim-format --no-default-features --target thumbv7em-none-eabi --locked
cargo test --manifest-path fuzz/Cargo.toml --locked
```

The embedded target must be installed before its check. Run Python validation
scripts from this root; native differential probes require their documented
upstream wimlib oracle, and Windows checks require prepared disposable guests.
UUP integration tools additionally use artifacts from the sibling checkout.
Existing evidence records describe the original executions, not a new validation
of this extraction. Host tests do not establish Windows installation correctness.

See [the library guide](crates/wim/README.md),
[implementation plan](docs/wimlib-rust-implementation-plan.md) and
[test strategy](docs/wimlib-test-strategy.md). The WIM crates are licensed under
LGPL-2.1-or-later OR GPL-3.0-or-later; both license texts are included here.

CI validates the portable workspace with a pinned sibling `ms-compress` checkout.
Repository variables `MS_COMPRESS_REPOSITORY` and `MS_COMPRESS_REF` select it.

## Publication

The portable workspace publishes `wim-types` and `wim-format` to crates.io.
The Windows capture/CDylib crate in `crates/wim` is a separate unpublished
workspace until `windows-disk` and `windows-ntfs` are ready for publication.
Its source and `disk-capture` feature are preserved. Validate that workspace
separately with `cargo test --manifest-path crates/wim/Cargo.toml --all-features --locked`
when its sibling Windows dependencies are available.
Version tags validate and publish the portable crates and create the GitHub Release.
