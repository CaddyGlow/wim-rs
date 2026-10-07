# Native Linux image extraction

`wimlib_extract_image()` now applies current native image metadata and resources
to a Linux filesystem. It does not call or link the original implementation.
The original library and unchanged public header remain external test oracles.

The backend creates directories, regular and empty files, sparse files, hard
links, and translated symlink/junction reparse points. It preserves WTF-8 names,
including unpaired UTF-16 surrogates, and supports reparse fixups. `UNIX_DATA`
restores uid/gid/mode/rdev, special files, and current/deprecated Linux xattr
formats. File and directory access/write timestamps are applied separately;
directory metadata is applied last. Linux ignores named streams, Windows file
attributes, unsupported reparse payloads, and EFS content according to the
original backend's policy. Strict security/short-name requirements are checked.

Resource reads use retained descriptors, source-independent referenced/exported
backing snapshots, bounded resource chunks, and rolling SHA-1. Recovery retries
failed decompression into a zero-initialized chunk and permits hash mismatches,
matching the original's two corrupted-file fixtures. Seekable split images
require referenced missing resources. Resource order follows GUID, part, and
resource/blob offsets, preserving tested progress sequences. Progress registration
is snapshotted for the extraction, with real begin/structure/data/metadata/end
events and original abort/unknown-status errors.

Image-derived names must be single safe components. Directory traversal uses
`openat()` with `O_DIRECTORY|O_NOFOLLOW`; file creation and replacement never
follow existing leaf symlinks. The parent-symlink regression proves that an
existing directory symlink cannot direct extraction outside the target.
The source WIM remains unchanged and may be unlinked after export.

## Reproduction and evidence

Run from the repository root:

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim
python3 scripts/wimlib/check-extract-api.py
cargo test --manifest-path Cargo.toml --target-dir target --locked -p wim --test extract
```

`probe-extract-api.c` compiles unchanged against `/tmp/wimlib/include/wimlib.h`
and separately links the original and native libraries. The harness allocates
disposable input/output directories. `differential.json` records **135 cases and
1,538 exact C observations, with zero mismatches**. Filesystem snapshots also
compare names, types, payload hashes, sizes, hard-link equivalence, uid/gid/mode,
xattrs, symlink targets, and timestamp nanoseconds.

The matrix executes all 17 named fixture commands from the original
`tests/common_tests.sh`, sourced unchanged by
`tests/test-imagex-capture_and_apply`, in none/XPRESS/LZX modes. It adds LZMS,
WTF-8 names, sparse files, multiple independent files sharing content, empty hard
links, Unix xattrs/FIFOs, existing-leaf replacement, all-image subdirectories,
flag errors, and abort/unknown statuses at several phases. Original malformed,
long-path, empty-DACL, old-xattr and corrupted-data WIM fixtures are included, as
are missing/referenced split resources and nonfirst-part metadata failures.

Snapshot reads use `O_NOATIME`, so reading one hard-link alias cannot alter the
timestamp subsequently observed on another. Absolute fixed symlink target
prefixes are normalized to `<TARGET>`. Only wall-clock creation timestamps on
unrestored nodes after failed operations and the newly created all-images
container directory are normalized; restored timestamps remain exact.
`expanded-red.json` retains the diagnostic run that exposed the need for this
failure-state timestamp normalization. During phase-abort development the probe
also caught two actual backend differences: creating the root before event 3,
and omitting original regular-file preallocation before event 4. Both are fixed.

The original oracle requires `WIMLIB_DISABLE_CPU_FEATURES=sse4.2` on this host,
as recorded by earlier original SIMD investigation. No input fixture or original
header/source is modified. Five native Rust regressions cover source-free exported
sparse/hard-link extraction, parent-symlink containment, selected paths, UTF-16
path lists, and rootless pending image behavior. A pending image without a root
fails extraction with error 49, matching the original. Caller-specified target
root symlinks are followed; image-derived directory symlinks remain rejected.

## Remaining gates

This is substantive **partial** C API compatibility, not full filesystem apply
completion. Windows/NTFS ACLs, alternate streams, native reparse handling,
attributes, compact mode, and WIMBoot need independent platform implementation
and Windows VM evidence. This Linux implementation uses `/proc/self/fd` for
path-only leaf metadata syscalls; other Unix platforms are not established.
Privileged ownership/device/security-xattr behavior needs disposable privileged
Linux coverage. Directory descriptors and concurrently extracted shared-content
file instances currently scale with image breadth; descriptor-limit stress and
bounded descriptor caching remain open. Whole input backing snapshots remain
buffered by handle opening. Custom allocator coverage follows the facade's
existing partial ownership scope.

Path and path-list extraction have separate evidence in
`../native-ffi-extract-paths/`. Pipe extraction remains unimplemented. This evidence
does not establish Windows servicing, installation, or whole-library replacement.

The backend now separates descriptor-anchored preparation, incremental stream
consumption, and final directory metadata application. `PreparedExtraction`
retains parsed metadata by borrowing the caller's storage; it does not own a
whole-archive buffer. Each `StreamSink` writes supplied decoded chunks directly
to regular files, buffers only bounded reparse payloads, checks the final digest,
and then applies file metadata. Ordinary image/path extraction uses this same
lifecycle. A fresh post-refactor run retained all 135 image cases / 1,538
observations and all 445 path cases / 3,177 observations with no mismatches.
This reusable backend alone does not establish live pipe API support; pipe
header selection, resource framing and callback contracts require separate tests.
