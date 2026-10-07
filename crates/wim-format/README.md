# Portable WIM core

`wim-format` supports `no_std + alloc` with default features disabled:

```toml
wim-format = { path = "../wim-format", default-features = false }
```

The portable core includes header and lookup parsing, metadata reading and
writing, XML editing, resource compression and decompression, integrity hashing,
in-memory archives, image construction and selection, repacking, solid and
pipable output, and split/join operations. `wim-types` and
`ms-compress` also support builds without `std`. Dependency feature forwarding
keeps `std` disabled throughout this graph when used in isolation.

The default `std` feature adds `file_archive`, `file_resource`, `pipable_read`,
`pipable_image`, and `integrity::calculate_file`. The higher-level `wim` crate
continues to provide hosted filesystem capture/extraction and the C interface.
Cargo features are additive: another dependency enabling `wim-format/std` will
enable these adapters for the whole build.

Consumers must supply a global allocator and their target's panic handler.
Shared ownership requires pointer-width atomics. This is not an allocation-free
library. Buffers and hash collections use checked, fallible growth; `Box` and
`Arc` allocations follow Rust’s global allocation failure policy. Custom C
allocation callbacks are no longer supported.

Validation:

```sh
cargo check -p wim-format --no-default-features --target thumbv7em-none-eabi --locked
cargo test -p wim-types -p wim-format --no-default-features --locked
cargo clippy -p wim-types -p wim-format --all-targets --no-default-features --locked -- -D warnings
```

The bare-metal check verifies compilation without `std`; host tests exercise the
portable configuration. Neither establishes bare-metal execution or Windows
installation/servicing correctness.
