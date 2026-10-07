# Native WIM opening with progress

`wimlib_open_wim_with_progress` now registers a caller-owned callback/context on
successful handles and emits real integrity-verification events during opening.
Events report the canonical filename, covered bytes, chunk size/count and
completed bytes/chunks. Missing integrity tables emit no events. Callback status
1 aborts with error 76; other nonzero statuses return 77. Failure releases pending
state and leaves caller output storage untouched. Ordinary `wimlib_open_wim`
delegates to this implementation with no callback.

`contract-red.log` records the unchanged-header C client's missing-symbol link
failure. `differential.json` compares 336 original/native cases across valid,
missing, malformed, truncated and mismatching integrity resources, bad XML and
short headers. Cases vary all relevant open flags, callback stopping positions
and invalid statuses. The client also checks null/empty filename and null output
validation. Exact callback field values and event order match, and input files'
SHA-256 values remain unchanged.

`integrity-contract-red.log` records the missing safe adapter;
`integrity-contract-green.log` covers nine format integrity tests. The added
adapter delegates to the existing chunk verifier, preserving initial-cancellation
precedence and suppressing events for absent tables. `contract-green.log` covers
two ABI regression tests for completed chunks, retained registration, cancellation
and unchanged output ownership.

Reproduce:

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim
python3 scripts/wimlib/check-open-progress-api.py --native target/debug
cargo test --manifest-path Cargo.toml --target-dir target --locked -p wim --test open_progress
```

This API remains partial. The current implementation buffers the whole input;
allocator hooks, allocation failure injection, concurrent input mutation/read
failure timing, global locale modes and Windows ABI/runtime gates remain open.
Callback code/context remain caller-owned. Event storage and filename pointers
are borrowed only during the callback. Registration replacement uses a `Cell`;
it does not authorize resource mutation or handle release during an operation.
No production original C wimlib code is linked.
