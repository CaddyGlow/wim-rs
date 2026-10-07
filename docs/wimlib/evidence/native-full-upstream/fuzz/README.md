The optional upstream randomized runner is distinct from the 72 public ABI exports. Original `tests/wlfuzz.c` requires `ENABLE_TEST_SUPPORT`; its native link failure is retained in `link-red.log`. The actual missing symbols are `wimlib_compare_images` and `wimlib_seed_random`. Resolving those names alone does not establish a working randomized test runner.

The original test support header also declares XML roundtrip and UTF conversion helpers. The random capture backend is an internal function, `generate_dentry_tree`, selected by `WIMLIB_ADD_FLAG_GENERATE_TEST_DATA` (`0x08000000`) only when optional support is compiled. The fuzz caller passes a function pointer cast as its source argument; this is deliberately not a real filesystem pathname. Standard production builds must reject the unsupported flag before interpreting that pointer as a string.

Source audit: `src/test_support.c` in unchanged original commit `cd5e231c348c255ae5088873b5a66ee0eb96fa07` stores a 64-bit seed and consumes a 48-bit LCG (`state=(state*25214903917+11) mod 2^48`, result `state>>16`). Faithful generation also requires matching RNG consumption through UTF-16 names, short names, SID/ACL/security descriptors, object IDs, Unix metadata, xattrs, reparse points, stream sizing/content generation, hardlink reuse, directory recursion and child iteration order. A reduced random tree generator would not reproduce these tests.

The original comparator is asymmetric. It matches visible filenames and short names, hardlink equivalence, platform-dependent attributes, actual security descriptors, semantic stream hashes, object IDs, timestamps, Unix metadata and normalized xattrs. It compares stored semantic stream hashes rather than delegating data reads to C. Unix, NTFS, Windows and ext4 flags permit specific documented losses; generic byte equality is insufficient. The native helper is staged separately until differential tests validate those contracts. No randomized-suite success is claimed at this stage, and the original source/scripts are preserved.

The feature-only native `test_support` module now implements seeded state and image comparison through native parsed graph/stream metadata. `compare-differential.json` records 2,025 matching C status observations: all ordered pairs of seven codec/solid fixtures and eight real upstream-generated seeded random trees across nine flag sets. `compare-fixtures-initial.json` retains the first 441 observations. The runner builds both C callers against unchanged original headers and freezes actual libraries; no original implementation is called by the native helper. Two Rust regressions cover changed stream/timestamp policies and changed hardlink equivalence. Temporary graph allocations do not yet use injected hooks, and full diagnostics/dirty-image side effects need separate parity coverage.

`generator-red.json` retains direct original/native generator results: original seeds 0, 1 and 2 write genuine random-tree WIMs successfully; native returns InvalidParam24. The production source pointer is never accepted as a fabricated filename. Those pre-implementation InvalidParam results remain preserved; the current optional single-image backend is described below. `runtime-red.log/json` retain the first genuine unchanged native randomized-runner failure: `wlfuzz.c:583`, newly created zero-image LZX WIM, write flags `0x2001` (CHECK_INTEGRITY plus SEND_DONE_WITH_FILE_MESSAGES), error68 Unsupported. This occurs before generation and reflects an existing explicit writer scope gate. `original-runtime.log/json` record a successful two-second unchanged original randomized run; it uses the upstream time/PID seed and is not claimed to reproduce the native operation trace. Resolving the two optional symbols is a link milestone, not a randomized-suite pass.

```sh
python3 scripts/wimlib/check-test-support.py
```

The first writer gate was implemented through real event26 lifecycle work, documented with360 C cases in native-ffi-write/done-with-file-final-interop.json. The exact zero-image LZX `0x2001` write now matches original status0/errno0 and288-byte output. `runtime-after-done.log/json` retain a fresh unchanged native randomized attempt: it successfully creates a LZX WIM using `0x2000`, then reaches `op__apply_and_capture_test` and fails at the genuine unimplemented generator with InvalidParam24. This later trace is time/PID seeded; it is not normalized into the earlier trace and is not a suite pass.

The optional generator now has a source-exact random core in
`wim/src/engine/test_support/random.rs`. `wimlib_seed_random` selects its shared
48-bit LCG state; individual draws hold no mutex across allocator or progress
callbacks. Local-state regressions verify the original six zero-seed transitions,
high-half-first 64-bit draws and overflowing initial state. The foundation initially left `GENERATE_TEST_DATA` rejected. The complete staged
graph port below now consumes these draws, while production builds continue to
reject the flag before examining fabricated source pointers.

The generator must retain original inode-table history, independently of the
public lookup-table order gate. It starts with64 buckets; the bucket is
`(ino * 0x9e37fffffffc0001 + devno) & (capacity - 1)` with wrapping arithmetic.
New shareable inodes enter the bucket head, and growth after `filled > capacity`
walks old buckets/chains and inserts at new heads. `select_inode_number` first
draws a bucket and then consumes one random boolean per visited inode before
falling back to a new32-bit inode number. A flat inode list would change both
hardlink selection and all subsequent random choices. Directories, reparse
points and inode number0 are outside the shared buckets.


The full native optional generator now stages `OwnedMetadata` and immediately
hashed payloads, preserving the original inode buckets, head insertion/growth,
recursive random consumption, short-name cache state, streams, SID/ACL bytes,
reparse fields, object IDs, Unix data and xattrs. The independent unchanged-source
primitive oracle records170 byte-digest/length/next-RNG cases in
`generator-primitives-original.json`; the committed TSV is a test-only golden.
`generator-graphs.json` and `generator-graphs-128.json` compare32 and 128 full
original-generated images, respectively. Each native graph is parsed and compared
using exact metadata policy, including hardlink equivalence/security bytes, and
every generated nonempty payload is compared byte-for-byte against the original
archive. Empty data remains a zero hash; first-hash insertion order is retained
separately from the staging map.

The optional `wimlib_add_image` branch accepts the ignored source pointer opaquely,
including `(void *)1`. Real generation occurs between scan9 and scan11 with the
original zero scan counters and copied phase registration. Memory descriptors
own actual registered-allocator `Collection` bytes through `Shared`; they do not
pretend to retain a source WIM. Their lookup raw location/compression fields are
zero. Hashed resources are visible at SCAN_END before the tree attaches, and
cancellation rolls back resources/index and the newly added image. Two native
regressions prove export/source-release memory lifetime and end-abort visibility
followed by an empty handle lookup.

`generator-api-initial.json` freezes native SHA
`141e0a5af4e69379e3f9d968816b86256b1e79508d6044887a8d3461a28b2ccb` for72
unchanged-header cases:8 seeds, ordinary/pipable/solid writes, continuation and
scan9/11 cancellation. All48 cancellation cases match complete observations;
all24 successful native archives verify with the original CLI and compare equal
with original `wimlib_compare_images`. Their ordered lookup rows remain24 initial
reds while the retained lookup index is integrated; the observations are preserved.
The unchanged native randomized caller now passes generation and its written
image comparison. `runtime-after-generator.log/json` preserve its next real
Unsupported failure at overwrite flags `0x5810`, including the pending `WIMLIB_WRITE_FLAG_NO_SOLID_SORT`
flag. A randomized-suite pass is not claimed.

Remaining optional scope includes WIMBOOT configuration embedding and multisource/ADD-command generation,
registered allocation of temporary generator graph/security/data staging vectors,
OOM rollback across all callbacks and Windows runtime generator proof. Ordinary
nonnull configuration now uses the real configuration reader before SCAN_BEGIN;
`generator-config.json` records all63 original/native cases equal, including valid
ignored filters, missing-file83 and malformed-configuration14. The preserved
`generator-config-red.json` records the earlier explicit68 gate. Standard feature-off capture still
returns24 before reading an invalid fabricated source pointer. Three other
optional header helper exports remain absent. No original C code is a production
dependency: source inclusion/static linking appears only in the bounded test oracle.

```sh
python3 scripts/wimlib/check-generator-primitives.py
python3 scripts/wimlib/check-generator-graphs.py --seeds 128 --output docs/wimlib/evidence/native-full-upstream/fuzz/generator-graphs-128.json
python3 scripts/wimlib/check-generator-api.py --seeds 8
cargo test --manifest-path Cargo.toml --target-dir target -p wim --all-features --locked --test generator_primitives --test test_support_generate
```


`generator-api-final.json` is the authoritative expanded ordered proof:288/288
complete C observations match for32 seeds and all nine lifecycle/write variants,
frozen native SHA `78f585537efd47aece4fe0a6ddfe062dfc5372948ce42e0ec93619acd8f56274`.
All96 successful native outputs independently verify and compare equal through
original wimlib. `generator-api-index.json` preserves an intermediate harness
failure: its96 successful rows differed only because original CLI warnings named
different chosen target paths. The corrected runner writes each library to the
same disposable target path and retains untouched archive copies for independent
graph comparison; it does not rewrite warnings or lookup order. The initial
unordered resource reds remain in `generator-api-initial.json`.

```sh
python3 scripts/wimlib/check-generator-api.py --seeds 32 --output docs/wimlib/evidence/native-full-upstream/fuzz/generator-api-final.json
```

`solid-sort-red.json` preserves the genuine24-case default-solid ordering and
NO_SOLID_SORT failure baseline (4 complete matches). The implementation now ports
solid.c's extension/basename heuristic, including empty extensions, NTFS uppercase
comparison and shortest hardlink alias names. It changes actual concatenation and
lookup offsets, and NO_SOLID_SORT disables that heuristic. Memory resources remain
unnamed and shared solid source resources retain sequential ordering. Compact
writes bypass sorting. `solid-sort-initial.json` preserves the next4 callback reds;
write.c's write_raw_copy_resources copies a shared resource once and then reports
each blob separately, charging compressed bytes only to its first descriptor.
The real copy path now follows that phase. `solid-sort-final.json` records24/24
complete matching observations with native SHA
`4b7ad42df31641f1f3ff2adab5c32516d94328215ceb8df94d84a37a73dbed0f`.
Best-effort sorting failure preserves sequential order and emits the original
warning. Temporary sorting allocation injection, external captured hardlink alias
names and broader multi-resource/OOM cases remain explicit gates.

```sh
python3 scripts/wimlib/check-solid-sort.py --output docs/wimlib/evidence/native-full-upstream/fuzz/solid-sort-final.json
```

`runtime-after-solid-sort.log/json` preserve the next actual unchanged randomized
caller run against an all-feature frozen native library, SHA
`5aa2b81198eaebbddfd7b65c23e55e2ce42ede16db09c121a16a87d913c776b0`.
It reaches38 iterations including generated apply/recapture comparisons, exports,
deletes and overwrites, then stops with source line1014 Unsupported on split flags
`0x4000`. This is a real remaining split flag gate; randomized completion is not
claimed and random time/PID seeds do not establish matched operation traces.

The source-backed split NO_SOLID_SORT fix is independently recorded under
[native-ffi-split-join](../../native-ffi-split-join/README.md).
`runtime-after-split-no-solid-sort.json` freezes native SHA
`8d3f1f91a9e5f52feea931d4c9634f062348cad856581204a2e6b3fdd5ef151e`;
both actual original and native unchanged callers finish a2-second run with0.
Native reaches22 iterations. This limited randomized run is distinct from full
interoperability or matched-trace proof: the caller's real time/PID seeds differ.
The reproducible runner preserves separate unchanged-source hashes, frozen
libraries, SONAME symlinks, link output, caller binaries and full operation logs.

```sh
python3 scripts/wimlib/run-original-wlfuzz.py --seconds 2 --output-prefix docs/wimlib/evidence/native-full-upstream/fuzz/runtime-after-split-no-solid-sort
```

The longer original10-second run finishes0; native stops on a real generated
apply/recapture stream mismatch, preserved in
`runtime-after-split-no-solid-sort-10s-native.log`. Original comparison of native
images7/8 confirms missing empty unnamed DATA on a non-directory symlink.
Source dentry.c's write_dentry_streams requires RP plus implicit empty DATA.
The captured writer plan now serializes both actual streams and remaps binding
slots coherently when publishing metadata. A Rust regression verifies ordinary,
repeated, pipable and solid output, alongside directory symlinks that require no
DATA slot. `symlink-stream-fix.json` retains the independently re-captured failing
fixture result: original comparison succeeds0 when access-time comparison is
explicitly disabled (flags13); flags9 retains a genuine access-time difference
because that old extraction tree was previously traversed. This limited audit
is not claimed as an unchanged full randomized pass.
The next actual random run reaches split flags0x5 and stops Unsupported, retained
in `runtime-after-symlink-stream.json` and its logs; its original baseline finishes0.
The subsequent real pipable split implementation is documented separately.

## Documented 60-second randomized gate

The newer [performance follow-up](../../performance/optimization.md) retains a
final optimized artifact that passes the same unchanged 60-second gate with 703
native iterations and 2,460 original iterations. It also preserves a subsequently
exposed orphan-solid split rejection, the regression and its source-backed fix.
The records below remain the earlier header-version milestone.

`runtime-current-60s.json` preserves the first longer debug run: original returns
0, native reaches the external 110-second timeout. Buffered output is incomplete;
this is neither an assertion failure nor a pass. The runner now line-buffers
stdout/stderr without changing upstream source, permits a separate process
timeout, records operation counts, and exits nonzero for failed compilation,
assertions or timeout. Upstream checks its seconds limit only between operations,
so the default process timeout allows the last operation additional time.

`runtime-release-60s.json` preserves the actual release-build failure at iteration
11: opening the archive after adding an empty image and overwriting with flags
`0x4900` returns error20. The original baseline completes 1,808 iterations with 0.
`solid-version-failure.wim` retains the 22 KiB failing archive, with its hash and
trigger in `solid-version-failure.json`. The independent original reader also
rejects it: "Uncompressed resource has size_in_wim != uncompressed_size".

The append writer retained solid resources but selected ordinary header version
`0x10d00`. Original `blob_table.c` deliberately ignores SOLID resource flags for
that version. Original `write.c` preserves the existing header version on append,
upgrading to `0xe00` when SOLID is requested. Native append now follows that rule,
including compact writes. `append_empty_image_preserves_solid_resource_version_without_solid_flag`
in `wim/tests/overwrite.rs` reproduces error20 before the fix and passes after
it. `solid-version-fixed.json` records the same eight successful API calls
against original and native, version 3584, and independent original CLI verification
of both resulting archives. Its embedded probe source permits replay.

`runtime-solid-version-fixed-60s.json` is the current passing unchanged-source
randomized gate. The frozen native release SHA is
`5d597f0246873bba8e42b924f74c82cc099f6f4eb7fd0b99c3545a13ac4de2fe`.
Both callers finish with status 0: native 200 iterations, original 6,212. All ten
Linux operation types are selected, including 67 native apply/recapture operations
and 12 native split operations. Selection counts include operations that return
early when no suitable archive exists; they are not individual scenario counts.
Time/PID seeds differ, so this is not matched-trace or performance evidence.
No upstream source, assertions or operation table was changed.

The all-feature workspace check passes 356 tests with 1 ignored, and strict
all-target/all-feature Clippy and formatting pass. This closes the documented
Linux `wlfuzz 60` gate, not exhaustive randomized compatibility or the separate
libFuzzer, Windows/WIMBoot, FUSE, direct-NTFS, OOM and platform gates.

```sh
cargo build --release --manifest-path Cargo.toml --target-dir target --locked -p wim --all-features
python3 scripts/wimlib/run-original-wlfuzz.py --seconds 60 --native target/release/libwim.so --output-prefix docs/wimlib/evidence/native-full-upstream/fuzz/runtime-solid-version-fixed-60s
```
