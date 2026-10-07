# wim-rs

Reusable Rust WIM implementation, extracted from windows-uup.

- `crates/wim`: native WIM API and C ABI, capture, extraction and writing.
- `crates/wim-format`: WIM parsing and serialization, with optional `std`.
- `crates/wim-types`: shared WIM types, with optional `std`.
- `scripts/wimlib`: differential probes and validation tools.
- `docs/wimlib`: retained validation evidence. Historical receipts are unchanged.
- `fuzz`: bounded WIM parser harness and honggfuzz executable.

Keep this checkout beside `../ms-compress`, `../virtdisk`, `../partmgr`,
`../disk-capture` and `../caddy-ntfs`. Compression comes from the standalone
ms-compress repository and the optional `disk-capture` feature uses the
standalone virtdisk, partmgr, disk-capture and caddy-ntfs repositories. UUP servicing and
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

CI validates the complete workspace using published compression and disk crates; no sibling checkout is required.
Repository variables `MS_COMPRESS_REPOSITORY` and `MS_COMPRESS_REF` select it.

## Publication

The workspace publishes `wim-types`, `wim-format` and the `wim` capture/CDylib
crate to crates.io. The optional `disk-capture` feature uses the published
`virtdisk`, `partmgr`, `disk-capture`, and `caddy-ntfs` crates.
Version tags validate and publish the engine crate and create the GitHub Release.
The already published portable format/type crates retain version 0.1.0.
