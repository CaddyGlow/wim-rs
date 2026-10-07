# Native image metadata serialization

`wim-format::metadata_write` adds two distinct serialization paths:

- `write_lossless(&Metadata)` validates and copies the complete original resource. Reserved bytes, invisible records, security padding, unpaired UTF-16 and malformed opaque tagged tails are preserved.
- `OwnedMetadata::encode()` constructs an aligned security table and directory tree, calculates all subdirectory offsets, sorts children in case-sensitive UTF-16 order, and serializes main/extra stream slots, short names, timestamps, inode unions and opaque tagged/security fields. The graph must have one unnamed directory root, unique visible sibling names, no repeated nodes or cycles, and no unreachable nodes.

`OwnedMetadata::from_metadata()` imports the visible reader tree and materializes canonical inode fields for hardlink aliases. Invalid security indices normalize to absent. Owned encoding is a canonical reconstruction, not byte-identical normalization of an arbitrary resource. Names are represented as UTF-16LE byte vectors; unpaired surrogates survive. Main and extra stream slots are explicitly modeled, allowing capture/update layers to choose wimlib-compatible stream ordering without interpreting opaque stream payloads here.

The layout follows preserved upstream `src/dentry.c` (`write_dentry`, `write_extra_stream_entry`, `write_dentry_tree`) and security-table encoding. Production code is native Rust with no C calls.

## Verification

Ten contract tests cover original lossless reconstruction, visible imported tree reconstruction, nested-offset calculation and hardlinks, opaque Windows security/tagged/reserved bytes, timestamps and short/stream names, and rejected cycles, duplicates, unreachable nodes and odd-length UTF-16 names.

`differential.json` records two genuine upstream capture/native metadata rewrite/upstream read-and-apply checks. Both normal and `--unix-data` images retain directory listings, pass upstream verification, and preserve file bytes, hardlink identity and symlink targets on upstream application. The original library is test-only `/tmp/wimlib-native-oracle/wimlib-imagex`.

Reproduce:

```sh
cargo test --manifest-path Cargo.toml --target-dir target --locked -p wim-format --test metadata_write
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim-format --example rewrite_metadata
python3 scripts/wimlib/check-metadata-write-differential.py --output docs/wimlib/evidence/native-metadata-writing/differential.json
```

This provides metadata construction, not filesystem capture, hardlink-ID assignment, Windows SD synthesis, public C ABI, or streaming allocation parity. The tests are not Windows installation evidence. `named-stream-red.log` records the red-first regression for the upstream named-stream terminator/alignment rule; `rust-tests.log` records all ten contracts passing after the fix. Other contracts were added after the initial implementation in this increment.
