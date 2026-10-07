# Native update transactions

`wimlib_update_image()` executes DELETE and RENAME command batches on native
metadata. Zero-command batches follow the original selection and dirty-image
rules. Commands use the unchanged public C layout, own canonical terminated
paths during callbacks, and emit events 21/22 around each command when requested.
Callback registration is reread at each boundary. Read-only traversal from a
callback observes changes already made by earlier commands.

Failure or cancellation restores the image metadata, blob reference counts and
dirty-image state. Callback registration changes remain in effect. No original
wimlib code is called by the implementation.

The unchanged-header caller `scripts/wimlib/probe-update-api.c` compares 5,400
cases against the preserved original library. `differential.json` records exact
stdout equality, including Linux x64 command/progress layouts, canonical paths,
callback events, intermediate and final trees, status and errno. Cases include
invalid images/operations/flags, successful and failed two-command batches,
root deletion, replacement, forced missing deletion, each cancellation boundary,
invalid callback statuses, callback unregistration and callback traversal.
The reserved rename field must be zero in valid callers; an exploratory nonzero
case also reproduces the original's copied-command zeroing behavior.

The initial missing-symbol failure, reserved-field mismatch, and callback errno
mismatch are retained in the red logs. Two Rust regressions check the measured
Linux x64 layout and restoration of metadata/blob counts at every command
boundary. A capture regression checks rollback of a newly attached graph when
a later DELETE fails, then checks successful retry with deferred streams and
zero hashes. An additional allocation regression fails the real canonical-path
buffer allocation, checks error 39 and unchanged image state, then retries
successfully. Canonical buffers and checkpoint collections use fallible Rust
allocation; these are not yet routed through registered C allocation callbacks.
The differential harness freezes the library before running and uses
disposable original-created source media.

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim --all-features
python3 scripts/wimlib/check-update-api.py --native target/debug > docs/wimlib/evidence/native-ffi-update/differential.json
cargo test --manifest-path Cargo.toml --target-dir target --locked -p wim --all-features --test update
```

ADD now dispatches to the native capture graph; its deferred stream consumers
and original-library differential gates are still being integrated. The 5,400
comparisons above establish DELETE/RENAME and zero-command behavior only.
Full allocator/OOM ownership, broader malformed metadata, named
streams, Unix errno transitions, Windows ABI and capture/apply platform gates
remain open. This is partial API evidence, not whole-library compatibility.
