# Repository guidelines

This Rust 2024 workspace owns `wim`, `wim-format`, and `wim-types`.
Published compression and disk dependencies resolve from crates.io; no sibling checkout is required.
Preserve the WIM crate licenses and historical validation evidence.

Use rustfmt defaults. Run `cargo test --workspace --all-features --locked` and
`cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`.
Also check `wim-types` and `wim-format` with `--no-default-features` when changing
them. Host tests do not establish Windows capture or installation correctness.
Run platform probes only in disposable environments; preserve original media.
