# Repository guidelines

This Rust 2024 workspace owns `wim`, `wim-format`, and `wim-types`.
Keep `windows-uup` and `ms-compress` checked out alongside this repository for shared dependencies.
Preserve the WIM crate licenses and historical validation evidence.

Use rustfmt defaults. Run `cargo test --workspace --all-features --locked` and
`cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`.
Also check `wim-types` and `wim-format` with `--no-default-features` when changing
them. Host tests do not establish Windows capture or installation correctness.
Run platform probes only in disposable environments; preserve original media.

The root workspace contains only crates ready for crates.io publication.
The capture crate is a separate unpublished workspace at `crates/wim/Cargo.toml`.
Keep the pending workspace sources and feature sets intact; validate it separately when its sibling dependencies are ready.
