# Windows extraction source baseline

The unchanged original Windows DLL was called through the public original
`wimlib.h` using a matching MinGW/MSVCRT caller in the owned disposable Windows
guest. The input WIMs were previously written by the native capture implementation
and independently verified and applied by the original Windows library. Input
bytes are snapshotted and checked after execution; each extraction uses a fresh
target directory.

`original.json` records the first 48 cases: default, absent and protected ACL
inputs; extraction flags; image selection; callback cancellation and invalid
status; NULL and conflicting filesystem targets. Inventories retain payload
SHA256, actual NTFS hardlink counts, raw attributes, creation/write FILETIME and
owner/group/DACL SDDL. The empty-target entry in this initial record is a caller
transport failure (exit 2 before entering the API), not an original API result.
The corrected caller uses an explicit sentinel to pass an empty wide string;
`original-complete.json` contains the corrected 48-case baseline.

`unsupported-red.json` preserves the native implementation before its Windows
backend work, with 38 differing cases. This is a substantive red baseline;
successful original extraction returned Unsupported in that native build.

`native-first-backend-red.json` retains the first implemented backend's ten
differences. `native-second-backend.json` matches all 48 corrected original
cases, including actual NTFS metadata and cancellation output state.

Two additional isolated sources contain 509 dentries, including 505 empty files
and a pair of six-byte hardlinks. The second source places an alias without a
DOS name before the alias with `PRIM~1.BIN`. Original structure and metadata
callbacks occur at counts 0, 500 and 509. Cancellation at structure count 500
leaves 499 filesystem entries: the original counts the earlier alias but delays
creating the entire inode group until reaching its DOS-bearing representative.
`extra-dos-native-red.json` and `extra-boundaries-native-red.json` retain the
missing-DOS and early-inode-creation failures. `extra-native-fixed.json` matches
the five actual source controls collected in `extra-original-combined.json`.
The latter combines the two named original records without rerunning or
inventing observations. Actual short-path filenames, hardlink counts, payloads,
attributes, permissions and timestamps are compared.

Default original extraction normalizes flags to RPFIX (256) and sends events
0, 3 (0/7), 3 (7/7), 4 (12/16 bytes, 1/2 streams), 4 (16/16, 2/2),
6 (0/7), 6 (7/7), and 7. All seven dentries count toward structure and metadata
progress. Callback abort and invalid status return 76 and 77. Conflicting ACL or
RPFIX flags, TOSTDOUT, unknown flags and NULL target return 24; invalid images
return 18; existing target or parent files send event 0 then return OPENDIR (48).
UNIX_DATA succeeds on Windows for these fixtures; NTFS mode returns Unsupported.

Run `scripts/wimlib/check-windows-extract-api.py --qga-socket SOCKET`, supplying
`--dll`, `--implementation`, `--baseline` and `--output` for native comparison.
Raw observations remain intact. Comparison excludes only wallclock timestamps
on partial operations, unchanged pre-created target files, and the synthetic
outer directory created when extracting all images. Restored image metadata
timestamps are otherwise compared exactly.

`probe-windows-extract-pipe.c` uses CreatePipe and matching MSVCRT
`_open_osfhandle(..., _O_BINARY)` descriptors. A bounded feeder writes actual
original-generated pipable WIM bytes. Remaining bytes are drained only after the
API returns, allowing read volume to be calculated without pretending a buffered
file is a pipe. The caller checks descriptor lifetime, binary mode and close.
The companion runner exercises plain and progress APIs, short fragments, image
selection, flags and cancellation. Pipe execution evidence is separate from
filesystem extraction evidence.

`pipe-original.json` establishes that the original Windows implementation closes
the supplied CRT descriptor. Its initial `api-read` field cannot distinguish
discarded queued bytes and must not be treated as an exact read measurement.
The strengthened observer probe retains a duplicated read HANDLE and drains it
after API return, before waiting for the bounded feeder, so queued bytes remain
observable even when the original closes the supplied descriptor.

`pipe-original-observer.json` records 26 corrected real-pipe cases. For the
3,576-byte original-generated input, success consumes 2,356 bytes and leaves
1,220; invalid image selection consumes 972. Cancellation at begin, structure or
part-begin consumes 2,260; the first stream consumes 2,312; metadata and end
consume 2,356. Seven-byte and 65,536-byte feeder fragments produce the same
read volume. Unknown flags fail before reading and leave the caller descriptor
open; entering the actual pipe operation closes it on success and failure.
NULL image selection succeeds for this single-image fixture. Part-begin event 5
occurs after structure progress and before stream progress.

These fixtures do not establish ADS, reparse, sparse/compressed files, EFS,
restricted-token behavior, remote filesystem behavior, or complete Windows
extraction compatibility. No servicing or installation claim follows from them.
