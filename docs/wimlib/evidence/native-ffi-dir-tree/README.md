# Native directory-tree C callbacks: partial implementation

`wimlib_iterate_dir_tree` now constructs the original public directory-entry prefix and flexible stream array from native decoded metadata. Callback storage, platform names, absolute paths, security descriptors, and resource entries live through each callback and are released afterwards. Callback return values stop traversal unchanged. Image selection supports ALL_IMAGES (-1); paths accept both separators and collapse repeated/trailing separators. CHILDREN omits the selected entry but still initializes and validates it before visiting children; RECURSIVE visits descendants in sorted UTF-16 order.

The unchanged-header C client checks 32 size/offset observations independently against a Rust layout example. On the verified Linux x86-64 ABI, directory entries are 248 bytes, stream entries 128 bytes, and the flexible stream array begins at byte 248. No inferred layout or substitute C structures are used in this check.

`differential.json` records 2,168 exact original/native client cases. Original-library captures cover all four codecs, two images, hardlinks, symlinks, UTF-8 names, Unix metadata, path selection, all flag combinations, invalid flags/images, and callback stopping. Split parts additionally cover missing stream resources and non-first-part metadata errors. Native-created opaque metadata fixtures are interpreted by both independent libraries and cover:

- Windows security descriptors, invalid security IDs, distinct DOS names on hardlink aliases, canonical inode timestamps, negative/very large timestamp seconds, and nanoseconds.
- Named data streams, reparse default streams with extra unnamed data, encrypted default streams, missing resource digests, and RESOURCES_NEEDED failures before callbacks.
- Unix UID/GID/mode/device tagged data and partial/oversized object IDs with original truncation to 64 bytes.
- Unpaired UTF-16 filename surrogates, exact WTF-8 callback names/full paths and selected paths, and invalid UTF-8 paths.

Three Rust regression tests read the actual flexible-array stream storage inside callbacks, check subtree/children traversal, preserve a negative stop result, and verify resource validation on a parent omitted by CHILDREN. `contract-red.log` records the missing native symbol. `empty-name-red.json` records an original/native mismatch corrected by returning nonnull empty strings for absent POSIX filenames/DOS names; the original header's NULL description does not describe this actual POSIX behavior.

The original optimized LZMS capture crashes in the known SSE4.2 encoder path. This matrix uses upstream's supported `WIMLIB_DISABLE_CPU_FEATURES=sse4.2` setting for original captures, with no source modification. The earlier original crash and generic-path evidence are recorded in the native C writing evidence. Native readers do not depend on the original library.

Reproduce:

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim --examples --lib
python3 scripts/wimlib/check-dir-tree-abi.py --native target/debug
cargo test --manifest-path Cargo.toml --target-dir target --locked -p wim --test dir_tree
```

The export remains **partial**. Windows callback layout, default case-insensitive path matching and wide strings need platform validation; only Linux x86-64 native C layout is proven. Global locale/encoding switches and allocator hooks/failure injection remain incomplete. Metadata is currently buffered in full, and several traversal/name allocations still use Rust's infallible allocation paths. External resource references and filesystem capture must supply their resource descriptors through handles before they can receive equivalent callbacks.
