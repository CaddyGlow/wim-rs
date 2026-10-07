# Mount API disabled-backend capability

The native build exposes the three mount/unmount C entry points and explicitly
has no FUSE backend. This is capability-gate compatibility, **not filesystem
mount support**. Each entry point returns `WIMLIB_ERR_UNSUPPORTED` (68) before
reading its arguments; unmount progress callbacks are not invoked. Diagnostics
use the existing native diagnostic sink. No mountpoint, staging directory, image
metadata or input WIM is modified.

The preserved original `src/mount_image.c:2552–2587` implements this exact branch
when `WITH_FUSE` is disabled. The original oracle's `config.log` records
`./configure --without-fuse --without-ntfs-3g --enable-test-support`, and its
`config.h:182` undefines `WITH_FUSE`. The host has no `/dev/fuse`. The unchanged
original `tests/test-imagex-mount:57–62` explicitly skips mounted-filesystem tests
when that device is not readable/writable; none of those mount tests is claimed
as passing here.

`link-red.json` preserves the native missing-symbol failure before implementation.
`differential.json` records **1,763 exact observations** against the unchanged
original C header/library, with frozen original/native library hashes. Cases
cover NULL, pending and opened handles; invalid/valid image indices; NULL,
empty, existing and missing mountpoint paths; NULL/empty/existing staging paths;
each mount flag and mixed/unknown/negative flags; all three unmount callback
statuses. Results include status, errno, callback counts, handle state and an
enabled diagnostic. Source WIM bytes and an existing target sentinel are
fingerprinted/preserved. This proves disabled-build precedence even for flags
which a FUSE-enabled build would reject with error 24.

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim
python3 scripts/wimlib/check-mount-disabled-api.py
```

## Required real Linux FUSE backend

A usable mounted filesystem remains required. A future backend must be an
explicit build capability; enabling it must replace the disabled gate only after
actual functionality and independent tests exist. The current implementation
has no feature which pretends to enable FUSE.

1. Establish a disposable Linux environment with `/dev/fuse`, a readable/writable
   device, the mount helper, and an independently built **FUSE-enabled** original
   oracle. Preserve the existing no-FUSE oracle. Run unchanged original
   `tests/test-imagex-mount` there to obtain mounted read-only/read-write, discard,
   commit, new-image and busy-file/force contracts; a host skip is not a pass.
2. Implement a native session and FUSE request transport (kernel protocol or a
   Rust FUSE transport), without invoking upstream C wimlib. Retain image and
   backing ownership across daemonization/session lifetime. Follow original
   `mount_image.c:2171–2234` validation order: arguments/flag mask, read-write
   filename and mutability, image selection, dirty-image prohibition, shared
   image prohibition, and append lock. Default named-stream access to xattrs;
   use `subtype=wimfs,default_permissions`, read-only and allow-other options.
3. Build stable inode/dentry/open-file ownership from actual metadata and
   hardlinks. Implement the operations listed at `mount_image.c:2130–2165`:
   attributes, lookup/open/read/release, directories, symlink/readlink, named
   stream/xattr interfaces, permissions/ownership/timestamps, and all mutable
   file/directory/link/rename/truncate/write operations. Decode resource ranges
   lazily and bound chunk memory, including solid-resource reads. Preserve raw
   names, NTFS ordering, Unix data, security policy and offset/errno behavior.
4. For writes, retain real temporary staging files and stream ownership, with
   bounded first-write copying and shared inode behavior, following original
   staging logic at `mount_image.c:866–960` and open/write handling. Reuse native
   metadata mutation and writer resource integration; do not recapture a mounted
   tree or cache whole archive payloads as a substitute. Preserve source bytes
   until explicitly authorized commit, and clean up staging on discard/failure.
5. Implement separate-process unmount control and progress delivery. Original
   `mount_image.c:2330–2550` uses mount-control xattrs and a POSIX message queue
   for commit progress. Compatibility requires the control semantics, flag
   validation, mountpoint identification, callback 16 and writer events, busy
   handles/force behavior, discard and commit/new-image/integrity/rebuild/
   recompress behavior, and cleanup after cancellation; matching the internal
   IPC mechanism is not itself a public requirement.
6. Test genuine mounted I/O against the enabled original: read offsets and
   codecs, concurrent/open hardlink aliases, named streams, sparse files, raw
   names and symlinks, operation errno, Unix metadata, daemon/client lifetime,
   read-only rejection, busy/force unmount, source locking, transaction failures,
   allocator failure, and commit outputs independently verified/applied. Retain
   explicit Windows unsupported behavior as a separate platform contract.

Until those steps pass, these three APIs can only be marked partial with this
specific disabled-backend scope. The fourth previously missing export,
`wimlib_reference_template_image`, is independent of mounting.
