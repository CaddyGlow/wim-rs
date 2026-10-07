# Native C file and descriptor writing candidate

`wimlib_write` and `wimlib_write_to_fd` now write ordinary, solid or pipable WIMs
from the current mutable handle. The writer consumes current image selection,
source mappings after deletion, newly added empty images, edited XML properties,
GUID/boot/readonly/rpfix settings and independent ordinary/solid compression and
chunk settings. Existing input metadata bytes survive without reconstruction;
payload references are counted from canonical streams and unreferenced payloads
are omitted. The original input is preserved.

An empty pending image acquires a real directory root when written, matching the
original writer's transition from 16-byte null-root metadata to 128-byte metadata.
Root timestamps use current Windows timestamp units. Its metadata digest becomes
available in the pending lookup entry after success; its reported original input
resource size remains zero. The original XML DIRCOUNT remains zero. New handles
without root TOTALBYTES retain the first written value, while an existing root
TOTALBYTES is restored, following the original XML writer's temporary edits.
The handle's input GUID and compression/header settings remain unchanged.

`contract-red.log` records missing-export failure. `contract-green.log` records
three Rust tests: validation, 12 codec/layout combinations with actual empty image
metadata, and nonseekable descriptor rules/ownership. The format solid writer now
also accepts separate ordinary and solid settings, preserving the original
single-settings wrapper. Solid headers advertise their own codec/chunk settings,
while the WIM header and metadata use ordinary settings.

`probe-write-api.c` compiles with the unchanged original public header and warnings
denied. `check-write-api.py` executes 240 original/native comparisons: new and
existing handles, all four ordinary codecs, ordinary/solid/pipable layouts,
unchanged/appended/deleted images, all or individual image selection, and paths or seekable caller-owned descriptors.
It compares status and resulting header behavior, verifies both outputs with the
original library, and applies every image in native outputs with the independent
original extractor. Source SHA-256 is unchanged. Output encoding sizes and root
creation timestamps can differ. `invalid-metadata-hash.json` verifies that a
metadata digest mismatch returns original error 21, rather than payload error 28.

Reproduce after building the native library:

```sh
python3 scripts/wimlib/check-write-api.py
```

## Original optimized-reader counterevidence

The default CPU original reader crashes on a 48-byte LZMS-compressed payload from
this valid source. It crashes for both original-produced and native-produced
files. Both are retained as `default-cpu-crash-original.wim` and
`default-cpu-crash-native.wim`. `original-cli-crash.log` records the GDB stack at
`find_next_opcode_sse4_2`, called from `lzms_x86_filter(size=48)` during verify.
The matrix therefore uses the original library's supported environment switch
`WIMLIB_DISABLE_CPU_FEATURES=sse4.2` for original C clients/CLI readers and writers.
The original source is unchanged. This is evidence for the generic reader
configuration, not a claim that the default optimized reader passed.

## Remaining drop-in gates

These are partial exports. Output is fully buffered, and all resources are
reencoded rather than reusing existing compressed bytes when possible. Full
compression-level tuning, parallel compression, bounded streaming memory,
allocator hooks, callbacks/progress, close-error propagation and transactional
failure timing are incomplete. The currently unimplemented external-resource omission, done-with-file
message and solid-sort flags return unsupported (68); unknown flags,
contradictory integrity/pipable flags and unsafe-compact return invalid parameter
(24). Rebuild/soft-delete/ignore-readonly are ignored for new-file writing as the
original documentation specifies. Fsync is performed when requested. Integrity
is omitted when no resource lookup entries exist, matching original behavior.

Caller descriptors are duplicated and the original remains open. Nonseekable
output requires pipable format without integrity. Nonzero initial seekable
positions and non-Unix descriptor APIs return unsupported. Windows GUID generation
is not implemented, so Windows path output requires RETAIN_GUID until that gate
is implemented. Out-of-memory rollback, error precedence after failed opens and
handle changes before failed encoding require additional differential tests.
A C overwrite transaction is a separate pending API.

`WIMLIB_WRITE_FLAG_SEND_DONE_WITH_FILE_MESSAGES` now dispatches real event26 for nonempty filesystem streams. It counts remaining selected streams per captured inode, emits one pathname for hardlinks, omits empty files and inline reparse payloads, closes the retained reader before notification and uses the current write phase's callback snapshot. Ordinary notification follows final resource/table/fallback output and precedes final stream progress. Solid notifications follow the actual chunk containing each completed blob. Duplicate notifications follow real hashing/deduplication; compressed duplicate lookahead occurs before the pending original's final compressed chunk is flushed. Callback cancellation prevents later writes/phases. Unregistering from event26 leaves the current phase snapshot intact and suppresses later metadata phases.

`done-with-file-red.json` retains all120 original/native Unsupported reds. `done-with-file-initial.json` retains nine real compressed-duplicate event-order failures; `done-with-file-lookahead.json` records their correction. The expanded authoritative `done-with-file-final-interop.json` contains360 cases with360 matching event/path/source-removal/registration/cancellation sequences and360 matching return/errno/existence results, using frozen native SHA `8ab929c895aa3ad82ed3545084c502b98b20dd888e59a31ce3303218816147a5`. All264 successful outputs from each library independently verify with the original CLI. Only151 cases match the complete raw observation, including encoded archive length. The other209 archive-length differences remain in the evidence: compression/metadata encoding and capture timing can affect length, so the callback milestone does not claim byte-size parity. No counts or error/file states were substituted to make these cases pass.

The precise first randomized writer failure (new zero-image LZX WIM, CHECK_INTEGRITY|SEND_DONE_WITH_FILE_MESSAGES `0x2001`) now returns0/errno0 and writes288 bytes in both implementations. The unchanged randomized native caller subsequently reaches its unimplemented test-generation flag and returns24; that new red is retained under native-full-upstream/fuzz. Randomized suite success is not claimed.

Four Rust regressions demonstrate source deletion at event26 without later reads, exact abort payload size65745 before later phases, and compressed duplicate notification order with deduplicated output/reference count2. Remaining scope includes injected allocation routing for path/progress temporary storage, lookup callback ordering, asynchronous/parallel compression, platform-specific stream paths and existing codec/metadata encoding gates.

```sh
python3 scripts/wimlib/check-done-with-file.py --output docs/wimlib/evidence/native-ffi-write/done-with-file-final-interop.json
cargo test --manifest-path Cargo.toml --target-dir target -p wim --all-features --locked --test write_done_with_file
```

Post-abort handle state was independently audited rather than inferred from completed output. `done-file-state-red.json` preserves50 cases with differing descriptor hash/refcount values, plus additional ordered lookup differences. `done-file-state-initial.json` and `done-file-state-modify.json` each record360 matching descriptor value sets and360 matching lookup/verify/retry statuses with native SHA `98c16e2ea0e435b7dafbafbc66aa819a071bc8672086b0270f84a286a0145c54`. These are explicitly value-set comparisons, not claims of identical callback row order:308 ordered descriptor sequences match, and52 mixed-image ordered differences remain visible in raw observations. The source-derived [lookup order plan](../native-ffi-lookup/order-plan.md) describes the retained table capacity, insertion and rehash history still needed; the public iteration contract does not specify ordering, but the observation remains an open gate.

Readiness follows original control flow: a unique uncompressed blob interrupted at event26 remains unhashed because the synchronous read-end did not complete. A compressed/solid read-end or successful collision prehash has already published its digest before event26. Prior raw blobs publish after successful final progress. Native updates the authoritative pending graph and current serialization at these boundaries without creating empty roots or refreshing XML stats, and releases locks before callbacks. The fourth regression compares raw versus XPRESS abort lookup hashes and subsequent verification after source modification: raw remains unhashed and verifies0; compressed preserves the real hash and verifies88. The modified-source C probe also confirms retry0 versus retry88, respectively.

```sh
python3 scripts/wimlib/check-done-with-file.py --post-state --modify-after --output docs/wimlib/evidence/native-ffi-write/done-file-state-modify.json
```

The retained blob index now resolves the earlier52 ordered lookup differences.
`done-file-state-index-caller-fixed.json` freezes native SHA
`10408709b14d4b021d8b71b217f3770c4199c75531e8ea3822913e4cb21924dd`
and records all360 matching event lifecycles, result/errno pairs, ordered descriptor
rows and post-state statuses. Complete stdout matches145 cases; remaining output
size differences are preserved. `done-file-state-index.json` retains an intermediate
caller formatting defect: returning invalid callback status before printing a
newline attached the result and encoded-size field to event26. The corrected
probe prints the newline before returning, so events and result fields are directly
observable without normalizing their values.
