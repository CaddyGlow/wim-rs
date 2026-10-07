# Native image metadata parser evidence

Source contract: `/tmp/wimlib/src/security.c:read_wim_security_data`,
`dentry.c:read_dentry`, `setup_inode_streams`, `assign_stream_types_*`,
`read_dentry_tree_recursive`, `inode_fixup.c:inode_table_insert`,
`metadata_resource.c:fix_security_ids`, and `tagged_items.c:inode_get_tagged_item`.
The Rust module is a source-guided independent implementation, not an FFI
wrapper. Production code neither loads nor calls the original library.

`Metadata::parse` borrows the uncompressed resource. Security descriptor
sizes and bytes, complete raw metadata, dentry fields, UTF-16 long/short names,
opaque tagged items, reserved bytes, stream names/digests, hard-link union,
and all three raw Windows timestamps remain available without lossy Unicode
conversion. `Node::inode` identifies the canonical node for a file alias;
`inode_entry()` resolves effective inode fields and `security_descriptor()`
normalizes invalid security IDs. Hard-link grouping follows sorted tree
traversal and merges only nondirectories with equal nonzero group ID and equal
unnamed data digest. Raw inode IDs remain available separately.

Directory traversal uses an explicit stack, checked ranges/rounded lengths,
ancestor offset cycle detection and upstream's 16,384 directory-depth bound.
It ignores unnamed, dot/dot-dot, embedded-NUL and duplicate child names,
retaining first duplicate; names are sorted by UTF-16 code unit. Non-directory
child offsets are ignored. Root names remain in the raw record for lossless
inspection; visible callers must treat the root name as empty. Main/extra
streams infer data/reparse/encrypted types using upstream's zero-main-hash and
unnamed-stream ordering rules. Malformed tagged tails are preserved, and
lookup returns absent rather than making metadata parsing fail.

## Original fixture

`fixtures/metadata-unix.bin` is the exact 792-byte uncompressed metadata
resource from original C imagex capture:

```sh
mkdir -p /tmp/metadata-native-source/sub
printf 'hello metadata\n' > /tmp/metadata-native-source/sub/file
ln /tmp/metadata-native-source/sub/file /tmp/metadata-native-source/alias
ln -s sub/file /tmp/metadata-native-source/link
/tmp/wimlib-native-oracle/wimlib-imagex capture \
  /tmp/metadata-native-source /tmp/metadata-native.wim \
  --compress=none --unix-data
```

Select the metadata flagged (`flags & 2`) 50-byte blob-table record from the
header's lookup resource; copy its stored resource span. Capturing again
changes timestamps, but semantic assertions do not fix their exact values.
The fixture contains root, hard-link aliases, symlink/reparse stream, nested
directory and Unix tagged metadata. C `wimlib_iterate_dir_tree` and the native
Rust client both report:

```text
/ attrs=16 links=1 streams=0
/alias attrs=128 links=2 streams=0
/link attrs=1024 links=1 streams=0
/sub attrs=16 links=1 streams=0
/sub/file attrs=128 links=2 streams=0
status=0
```

## Differential and regression tests

Ten `tests/metadata.rs` tests pass: fixture fidelity, SD range/size validation,
name filtering and sorting, cycle rejection, unchecked name terminator
compatibility, named/unnamed stream ordering, truncation at every one of the
792 byte boundaries, checked length overflow, consistent/inconsistent file
hard links and rejected directory hard-link merging, tagged-item lookup and
malformed tails, encrypted stream inference, and empty image acceptance.

Additional executable oracle tooling is retained under `fixtures/`:
`metadata-oracle.c` opens the original WIM and iterates its tree;
`metadata-client.rs` prints corresponding native tree fields;
`metadata-differential.py` mutates the original uncompressed metadata,
recomputes its blob SHA-1, and compares complete stdout between both clients.
The native test client prints path-error 49 for an empty tree because original
iteration of `/` returns PathDoesNotExist even though metadata parsing succeeds.

40 cases matched on 2026-10-03: baseline; security total lengths/counts; root
attributes, record lengths, name lengths, stream counts and child offsets;
child security IDs, attributes, name lengths and hard-link group IDs. Cases
include accepted unusual layouts and malformed-buffer errors, rather than
only generated valid layouts. Initial differential failure was the test
client's empty-image iteration behavior (status 0 vs original status 49); the
client was corrected without changing the native parser.

```sh
cc -I /tmp/wimlib-native-oracle/include \
  crates/wim-format/tests/fixtures/metadata-oracle.c \
  -L/tmp/wimlib-native-oracle/.libs \
  -Wl,-rpath,/tmp/wimlib-native-oracle/.libs -lwim \
  -o /tmp/metadata-native-tree
# Compile metadata-client.rs against cargo's built libwim_format rlib using
# rustc --extern wim_format=... -L dependency=<cargo target>/debug/deps.
python3 crates/wim-format/tests/fixtures/metadata-differential.py \
  /tmp/metadata-native.wim /tmp/metadata-native-tree /tmp/metadata-native-rust
cargo test --manifest-path Cargo.toml -p wim-format --test metadata
cargo clippy --manifest-path Cargo.toml \
  -p wim-format --all-targets --all-features --locked -- -D warnings
```

## Limits of this evidence

The module decodes already-read metadata; whole-resource SHA-1 verification,
512x decompression allocation guard, editable graph serialization, filesystem
capture/apply and the public C ABI belong to other layers. It retains Windows
security/reparse descriptors as bytes and exposes tags without validating
Windows ACL internals. It has no output-image writer. Checked integer alignment
rejects malicious u64 wraparound instead of reproducing C overflow behavior.
Shared noncyclic child lists are permitted and can expand graph work; there is
not yet a caller-configurable node/allocation budget. Cycles are rejected
before upstream's depth-only cycle check would exhaust its traversal limit.
These tests do not establish complete Windows metadata or ABI compatibility.
Tests were added in the same implementation iteration; the first fixture test
failure was an incorrect assertion about the zero main symlink hash, corrected
to locate the inferred reparse stream. This is recorded as a limitation of
strict test-first provenance for this slice, not a claimed full TDD campaign.
