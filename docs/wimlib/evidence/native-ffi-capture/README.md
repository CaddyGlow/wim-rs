# Native capture integration in progress

Capture retains an owned metadata graph and explicit stream bindings. Regular
files retain their source path and scan-time size; scanning does not read or
hash their payloads. Hardlink bindings share actual stream ownership. SHA-1
fields remain zero until content is hashed; internal stream identities are not
stored in digest fields. Metadata traversal maps serialized nodes back to the
authoritative graph using the encoder's directory ordering.

`inspect-differential.json` records 24 exact unchanged-header C comparisons of
lookup and directory traversal immediately after capture. The fixtures contain
hardlinks or independent identical files, an empty file and a nested directory.
Both callbacks are stopped at several boundaries. Entries compare scan sizes,
reference counts, resource flags, zero hashes and callback return codes. The
harness freezes the native library and fingerprints source file contents before
and after inspection. No output WIM is written in this test.

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked --all-features -p wim
python3 scripts/wimlib/check-capture-inspect.py --native target/debug > docs/wimlib/evidence/native-ffi-capture/inspect-differential.json
```

`source-contracts.json` and `dedup-contracts.json` retain original-library scan,
deferred-read, write and deduplication observations used to drive integration.
They are reference contracts, not evidence of native passes. Four Rust scanner
regressions cover deferred content changes, missing/truncated files, hardlink
ownership, configuration exceptions and symlink fixups. Update regression
coverage checks that a subsequent failed command restores the pre-capture image,
and that the reparse-fixup header flag is committed only after batch success.

Public capture wrappers and update ADD dispatch are being integrated with
writing, export, verification and extraction. Full scan callback/error policy,
configuration, capture-write-original-read comparisons, allocator routing,
bounded I/O and Windows/NTFS capture remain required gates. This inspection
milestone does not establish full capture API compatibility.

`scan-differential.json` records 182 cases and 1,010 exact original observations
for scan/state behavior, including configuration, flag checks, symlinks, missing
sources, scan cancellation and exclusion decisions. Regular stream hashes remain
deferred; small translated reparse payloads are hashed during capture, as in the
original. Header reparse-fixup state is committed only after batch success.

The shared captured-resource resolver now handles both unhashed ownership and
real hashes published by writing. Lookup coalesces hashed duplicate content;
verification reads the retained source and checks its real digest. The separate
`verify-differential.json` records 36 exact cases before/after writing, at 19,
32,768 and 65,537 bytes, with independent files, hardlinks and duplicate content.
Post-write mutations cover changed, truncated and missing source files. Native
status and verification progress match the original. `verify-red.json` and
`verify-stream-red.json` preserve the missing-resolver and subsequent failure
behavior mismatches. These disposable mutation fixtures deliberately modify only
their test inputs.

```sh
python3 scripts/wimlib/check-capture-verify.py --native target/debug > docs/wimlib/evidence/native-ffi-capture/verify-differential.json
```

Capture export now retains pending graph ownership for hashed streams as well
as unhashed streams. `export-differential.json` records 24 export cases with
ordinary files, hardlinks and symlinks, before/after source writing, and name,
description and GIFT policies. Source handles remain live for ordinary exports;
GIFT cases release the source before destination verification and writing.
All 24 native outputs pass the original reader and application. Fifteen paired
outputs match statuses and applied content, modes, symlinks and hardlink groups.
In the other nine, the original returns success but its own reader rejects its
output with error 55; these post-write non-GIFT export cases remain an explicit
oracle limitation, not an exact-parity claim. `export-red.json` preserves the
native missing-resource failure that prompted the retained-graph fix.

Export computes pending source and destination stream checksums after duplicate
image checks and before name-collision checks, following the original source.
`export-errors.json` adds nine exact cases with missing, truncated or replaced
source data and ordinary/name-suppressed/GIFT exports. Missing/truncated files
return 47/88 before adding a destination image; newly changed content is hashed
from the actual scanned-size prefix.

```sh
python3 scripts/wimlib/check-capture-export.py --native target/debug > docs/wimlib/evidence/native-ffi-capture/export-differential.json
```

Captured writing now reads regular files lazily after the initial WRITE_STREAMS
callback. The ordinary, pipable and solid writer matrices each record 44 exact
C observations in `write-differential.json`, `write-pipable-differential.json`
and `write-solid-differential.json`. They include missing, shortened, replaced
and grown scan sources. Successful outputs pass the original verifier and
application and reproduce the source tree. Each matrix freezes its native
library and records its SHA-256; independent matrix runs can use different
artifacts while parallel integration proceeds.

Hash publication rebuilds authoritative pending metadata at the real metadata
phase. Empty captured graphs materialize a root before encoding. Independent
same-size duplicate files retain two unhashed descriptors until data processing;
ordinary and solid writing then coalesce their real hashes and reference counts.
`solid-dedup-red.json` preserves the former incorrect completed-stream counts and
larger resource; `solid-dedup-green.json` records equal callback counts, output
sizes and successful original verification. Metadata hashes differ because the
separate captures contain independently generated timestamps. Three focused
Rust regressions cover ordinary deduplication, solid deduplication and shortened
source failure timing.

```sh
python3 scripts/wimlib/check-capture-write-api.py --output docs/wimlib/evidence/native-ffi-capture/write-differential.json
python3 scripts/wimlib/check-capture-write-api.py --write-flags 4 --output docs/wimlib/evidence/native-ffi-capture/write-pipable-differential.json
python3 scripts/wimlib/check-capture-write-api.py --write-flags 4096 --output docs/wimlib/evidence/native-ffi-capture/write-solid-differential.json
```

These cases do not establish all compression strategies, multi-threaded write
behavior, allocator-failure parity or every mixed source-resource deduplication
and cancellation boundary. The broader writer and overwrite evidence retains
those independent gates.

`multisource-differential.json` adds 542 cases and 3,378 exact observations for
`wimlib_add_tree`, `wimlib_add_image_multisource`, and update ADD batches. Sources
include cross-command hardlinks, empty hardlinks, directory merges, file
replacement and type conflicts. The matrix checks root/nested/canonical targets,
reserved fields, image-name collisions, boot flags, capture configuration,
NO_REPLACE, rollback after later failure, and scan/update/replacement/exclusion
and error callbacks. Both successful output WIMs are applied by the unchanged
original CLI and their filesystem results compared. Synthetic parent timestamps
are normalized because each capture creates them independently; scanned source
metadata is fixed. Same-update inode identities retain deferred shared streams;
independent update sessions have independent inode identities.

`apply-differential.json` records 68 cases and 1,102 exact observations using the
17 named fixtures from unchanged original `tests/common_tests.sh`, with ordinary,
verbose, Unix-data and verbose Unix-data capture. Successful native and original
outputs are independently applied using the original CLI. Applied metadata,
payloads, symlinks and hardlink topology match; source payloads remain intact.

`direct-extract-differential.json` records 40 cases and 1,180 exact observations
for extracting pending capture graphs before writing, including duplicate regular
files, hardlinks, relative/absolute symlinks, extraction flags and callback abort
or invalid status. Checksumming reads scan-size prefixes in bounded buffers,
canonicalizes duplicate stream ownership, and retains real source paths rather
than caching file payloads. The subsequent write also matches original progress.
Aborted application timestamp fields are normalized because unfinished files
retain independently generated host times. `direct-extract-red.json` preserves
the duplicate-ownership mismatch that preceded this fix.

```sh
python3 scripts/wimlib/check-capture-multisource-api.py
python3 scripts/wimlib/check-capture-apply-api.py
python3 scripts/wimlib/check-capture-direct-extract.py
```

These are partial Unix capture API claims. Remaining gates include Windows and
NTFS capture, snapshot support, privileged device/security cases, source races,
very deep/long or cyclic dereferenced source trees, configured-allocator failure
coverage, pending-stream checksumming failure-state parity, and inode reuse after
mixed ADD/DELETE/RENAME commands remove an earlier scanned inode from the graph.
Existing source descriptors and captured streams with equal hashes require
broader mixed-resource differential coverage. Live pipe extraction remains a
separate backend task.

The Windows scanner integration factors the existing Unix capture code through
shared platform-text/configuration adapters. `platform-refactor-scan.json` records
a frozen native artifact with 182/182 cases and all 1,010 original scan/state
observations matching after this refactor. The earlier loader-only red is retained
separately: the frozen SONAME15 library initially lacked its required
`libwim.so.15` symlink, so every native probe exited before entering the API.
