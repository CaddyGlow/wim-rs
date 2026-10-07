# Native Linux selected-path extraction

`wimlib_extract_paths()` and `wimlib_extract_pathlist()` select real image metadata
and use the native filesystem backend. Neither calls or links upstream wimlib.
Literal paths preserve ancestors by default; `NO_PRESERVE_DIR_STRUCTURE` flattens
selected roots while retaining subtree structure. Selection removes redundant
subtrees and retains hard-link relationships among selected entries. Globs use
the original component rules, strict no-match behavior, and configured case
sensitivity. `TO_STDOUT` writes selected regular streams in caller order,
including duplicates, without extraction progress events.

Path lists support UTF-8/UTF-16 BOMs, quotes, comments, whitespace, and a final
line without newline. A NULL path-list filename reads stdin; the literal filename
`-` refers to a disk file, unlike the public general text loader's dash policy.
Selection, validation, errno, failure output state, callback ordering, aborts,
and invalid callback statuses are compared against the unchanged original API.

## Reproduction

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim
python3 scripts/wimlib/check-extract-paths-api.py
cargo test --manifest-path Cargo.toml --target-dir target --locked -p wim --test extract
```

`probe-extract-paths-api.c` uses the unchanged original header. The disposable
comparison harness records **445 cases and 3,177 observations with zero
mismatches** in `differential.json`, plus filesystem snapshots of names, content,
types, timestamps, ownership, xattrs, and hard-link equivalence. Source WIM bytes
are fingerprinted before and after extraction. The matrix includes raw WTF-8,
invalid text, NULL parameters, pending rootless images, existing filesystem
conflicts, containment, stdout, solid/pipable resources, original malformed and
Unix-data fixtures, recovery, and solid resources with 32-KiB chunks. The latter
exposed progress chunk boundaries relative to shared-resource offsets; the
retained red evidence is `solid-chunks-red.json`. Other red artifacts document
earlier errno, stdout, pending-root, and target conflict corrections.

Snapshot normalization is limited to wall-clock timestamps on nodes whose
metadata was not restored after failure, unselected container timestamps, and
caller root symlink access time caused by traversal. Restored timestamps remain
exact. The external original oracle uses `WIMLIB_DISABLE_CPU_FEATURES=sse4.2`
for the previously documented host SIMD issue. Original inputs are preserved.

## Scope and remaining gates

These are substantive partial Linux C API implementations. They inherit the
backend limitations documented in `../native-ffi-extract/README.md`: privileged
Linux metadata, descriptor-limit scaling, custom allocation ownership, buffered
handle input, and other Unix platform evidence remain open. Windows/NTFS apply,
security descriptors, alternate streams, compact mode, WIMBoot, and genuine pipe
streaming extraction require further implementation and independent platform
evidence. This does not establish whole-library compatibility or Windows
installation correctness.
