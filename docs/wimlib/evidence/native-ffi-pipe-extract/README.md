# Pipe extraction reference contracts

`source-contracts.json` preserves 1,080 unchanged-header observations from the
original library. Inputs are actual subprocess stdin pipes, with original-created
pipable and ordinary fixtures, empty input, truncated headers, truncated XML, malformed frame magic/flags, inconsistent
image counts and part ordering.
The matrix checks NULL/numeric/all/missing image selectors, extraction flags,
normal progress, aborts and invalid callback statuses. It records target creation,
output paths, result/errno, progress totals and caller descriptor state. These
are reference observations. `differential-baseline.json` now records 1,080 exact
native/original comparisons, including status, errno, callbacks, descriptor state,
and filesystem output. Both Linux C entry points are partially implemented.

```sh
python3 scripts/wimlib/check-pipe-extract-contracts.py > docs/wimlib/evidence/native-ffi-pipe-extract/source-contracts.json
```

The source path is `extract.c:1986–2107`: validate public flags; open and validate
the first header; require pipable magic and part 1; consume the immediately
following XML blob; validate image counts and resolve exactly one image; consume
each image metadata blob in order; then extract data from the live stream.
Resource headers are 40 bytes (`resource.h:318`), with blob magic, uncompressed
size, SHA-1 and flags. Compressed chunks have a four-byte size prefix. A seekable
ordinary WIM supplied on a pipe returns error 44; short input returns error 65.

The original takes ownership of the input descriptor once open begins and closes
it on completion/error; invalid public flags return before taking ownership.
Successful extraction emits image-begin, structure, split-part-begin, actual data
chunk progress, metadata and image-end messages. Progress exposes public flags, including the default reparse-fixup flag for
the retained fixture; the private FROM_PIPE bit remains internal. Metadata/image selection failures precede target
creation; cancellation later in extraction can leave partial filesystem state.

The native implementation requires a bounded sequential reader, fragmented-input
tests, actual XML/metadata parsing, all codec chunk layouts, content checksums,
split-part transitions and recovery policy. Integrating the filesystem backend
must preserve callback/read ordering and cancellation without consuming the rest
of the pipe. A whole-input buffer or temporary WIM adapter would not establish
those stream semantics. Windows/NTFS, custom allocation and descriptor/error
ownership gates remain required in addition to these host contracts.

The native `pipable_image` preflight now uses the sequential reader to load XML,
resolve exactly one image, decode/skip each image metadata resource, and retain
only the selected metadata. XML/resource collection storage uses the supplied
allocator. File payloads remain unread. Three regressions verify selection for
all four original storage modes, invalid-selector failure before metadata reads,
and the original metadata digest error 21. The Linux C entry points now integrate this preflight with the shared filesystem
backend and incremental stream sinks.

The `wim-format::pipable_read` foundation now consumes genuine 40-byte blob
headers, four-byte compressed chunk headers, raw payloads and trailing offset
tables through `Read` alone. It retains reusable bounded buffers and native codec
contexts through an injectable allocator. Selected-data completion and callback
abort leave subsequent bytes unread; no whole-input buffering or temporary file
adapter is used. Initial headers are validated, while later part headers expose
raw identity fields exactly where the original payload loop allows them.

`reader-fixtures.json` records original verification/application of none, XPRESS,
LZX and LZMS fixtures. `codec-fixtures.json` records the additional original
capture commands and fixture hashes. Rust tests decode each fixture through
1-, 7- and 4096-byte fragmented reads with interrupted system calls, compare the
known original payload, and assert exact resource-end positions. Other tests
cover cancellation, truncation, retained OS errors, a 1 TiB logical raw resource
using a 32 KiB buffer, injected allocation failure before payload reading and
later-header validation boundaries.

`unused-metadata-original.json` records ten unchanged-header C cases for a
compressed two-image fixture. Skipping unselected metadata still decompresses
its chunks: invalid Huffman data and invalid chunk framing fail for either image
selection. A valid compressed stream producing corrupted metadata is ignored
when unselected; selected metadata fails with error 21. A SOLID flag without
COMPRESSED yields the upstream invalid-chunk-size error before reading payloads.
The reader regressions retain these distinct framing, parsing and hash policies;
metadata hash errors must be mapped to error 21 by image orchestration.

```sh
cargo test --manifest-path Cargo.toml --target-dir target -p wim-format --test pipable_read --locked
```

The baseline differential establishes the partial Linux export claim. Extended
fragmented-input and all-codec comparisons retain a real initial red: raw streams
reported 64 KiB write progress instead of the original 32 KiB raw reads. The
raw reader now uses the original `BUFFER_SIZE` of 32 KiB, preserving cancellation
read volume as well as progress boundaries. Native
integration regressions exercise 7-byte input fragments, descriptor closure,
cancellation before payload reads, and unread footer preservation. The expanded differential below confirms these boundaries; the remaining
platform and allocation gates still require separate evidence.

`part-differential.json` records 192 exact comparisons using genuine split parts
created by the original API. All four codecs cover part transitions, first/second/
last part callback cancellation and invalid statuses, omitted final parts, repeated
original part headers, and 7-/65536-byte input fragments. Comparison includes
actual API read volume and extracted content, metadata and hardlinks. The fixture
and frozen-library hashes remain in the evidence record.

The frozen native C lifecycle matrix now passes 1,836/1,836 contract comparisons
and 1,836 stderr comparisons in `differential-io-final.json`. Its library SHA-256
is `a34cebe8d1ac0d977e0468615002c0f71031ceea7920efc8005bd753e5c85606`.
This extends the original 1,080 cases with all four compression types, fragmented
stdin writes, callback abort/invalid statuses, Unix modes, real hardlinks and
symlinks, two-image metadata corruption and 64 KiB XPRESS/LZX plus 128 KiB LZMS
chunks. Every comparison includes return/errno, caller descriptor ownership,
callback payloads, actual filesystem contents/modes/link groups and exact bytes
consumed by the API.

The companion IO observer duplicates stdin before calling the API and drains the
duplicate only after the API has returned. Subtracting drained bytes from the
input size measures actual API consumption without changing reads or callbacks
during extraction. Descriptor assertions are captured before the observer drain.
The runner freezes both libraries before compiling unchanged-header clients and
records each payload's SHA-256. Fragmentation uses real subprocess pipes and
bounded writer blocks; the Rust reader tests separately force each read fragment.

`differential-io-initial.json` and `differential-io-expanded-red.json` retain the
incorrect raw 64 KiB callback/read boundaries. The intermediate
`differential-io-split-only-red.json` retains a correction that fixed callback
sizes but still read ahead. The source-derived final correction uses the
32 KiB BUFFER_SIZE in upstream `util.h:39` for raw reads and retains complete
archive-defined compressed chunks. Callback cancellation therefore prevents
future reads and writes, including for compressed chunks exceeding 32 KiB.
`large-chunk-fixtures.json` records original generation and independent
verification/application of the larger chunk fixtures. Eight safe reader tests
pass, including those callback/read bounds.

```sh
python3 scripts/wimlib/check-pipe-extract-api.py --baseline-only --output docs/wimlib/evidence/native-ffi-pipe-extract/differential-baseline.json
python3 scripts/wimlib/check-pipe-extract-api.py --observe-io --without-recovery --output docs/wimlib/evidence/native-ffi-pipe-extract/differential-io-final.json --reference docs/wimlib/evidence/native-ffi-pipe-extract/differential-io-final-original.json
```

The final combined matrix in `differential-final.json` passes 2,268/2,268
contracts and 2,268 stderr comparisons, frozen at native SHA-256
`54f5828372ac6188a214f1c9b597ca9388c189e423f5f9954356b632c3d72d73`.
It includes 432 default-versus-RECOVER_DATA cases for raw hash mismatches,
XPRESS/LZX/LZMS corrupted bodies, invalid chunk sizes, truncated prefixes,
truncated bodies and trailing tables, malformed blob headers and metadata-flagged
payloads. Callbacks abort or return invalid statuses during actual data work;
partial output contents and exact input consumption are compared without
normalization. Recovery tolerates codec/hash corruption where upstream does;
it does not suppress framing or input truncation errors.

`differential-recover-red.json` preserves 48 mismatches caused by treating a
matching-hash METADATA frame as file data. The corrected payload loop requires
the frame to be non-metadata, as original `extract.c:317` does, then skips it and
fails at EOF if required data never arrives. `differential-final-original.json`
preserves all original observations paired with the final comparison.

```sh
python3 scripts/wimlib/check-pipe-extract-api.py --observe-io --output docs/wimlib/evidence/native-ffi-pipe-extract/differential-final.json --reference docs/wimlib/evidence/native-ffi-pipe-extract/differential-final-original.json
python3 scripts/wimlib/check-pipe-extract-api.py --observe-io --recover-only
```

Split-part transitions have their separate 192-case evidence above. Windows/NTFS,
allocator-failure and remaining flag gates still require separate evidence;
these Linux matrices do not establish them.

The separate Linux target/flag policy matrix passes 4,440/4,440 contracts and
stderr comparisons in `differential-policy-final.json`, frozen at SHA-256
`894de573d85e0808af131d6cd9358f3587e382f857c9a36845b376fd0b704f63`.
It checks all public flag bits, reserved bits and contradictory combinations
against valid/invalid images, truncated headers and ordinary input. Targets
include actual NULL/empty pointers, missing parents, existing files/directories,
nonempty/read-only directories and existing/dangling directory symlinks. Existing
test sentinels are compared and preserved; only disposable targets are modified.
Read-only behavior records this process's actual permissions and does not claim
privilege-independent access-control enforcement.

`differential-policy-red.json` preserves the first 64 failures. Full-image
TO_STDOUT, GLOB_PATHS and NO_PRESERVE_DIR_STRUCTURE flags must fail after pipe
metadata preflight, before backend work. NULL/empty targets must fail before
NTFS/WIMBoot/Compact platform rejection. The corrected sequence matches original
`extract.c:1931` and `do_extract_trees:1782`. A native regression checks no backend
callbacks or target creation and exact unread tail after these failures; another
checks metadata-flagged matching hashes are skipped through EOF without creating
a payload file. Four native pipe regressions pass.

```sh
python3 scripts/wimlib/check-pipe-extract-api.py --policy-only --output docs/wimlib/evidence/native-ffi-pipe-extract/differential-policy-final.json --reference docs/wimlib/evidence/native-ffi-pipe-extract/differential-policy-final-original.json
```
