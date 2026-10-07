# Upstream wimlib tests and Rust replacement acceptance strategy

This is a source audit and implementation specification, not a claim that either upstream or a Rust replacement has passed these gates. The audited source is `/tmp/wimlib`, Git commit `cd5e231c348c255ae5088873b5a66ee0eb96fa07`. Paths below are relative to that tree. Keep this revision, configuration, tool versions, platform and fixture checksums in every oracle report. Upstream explicitly describes `tests/test-imagex` as sanity testing rather than comprehensive coverage.

## Executable inventory and upstream coverage

`Makefile.am:307–342` builds `tests/tree-cmp` for checks. Unix `make check` runs three unconditional suites and adds mounting under `WITH_FUSE` and direct NTFS under `WITH_NTFS_3G`. Native Windows deliberately sets `TESTS` empty; a successful Windows `make check` proves no Windows functional suite ran. `tests/wlfuzz` is an extra program and is not run by `make check`.

| Artifact | Behavior and matrix | Requirements / limits |
| --- | --- | --- |
| `tests/test-imagex` | None/LZX/XPRESS capture/apply; SHA-1 verification with cumulative CPU features disabled; append creation; metadata/name/description/boot flags; integrity table; append/delete including failures; multi-image apply; split/join; export and recompression transitions | Bash, coreutils/BSD stat, diff, built CLI; unreadable-file test only runs as nonroot |
| `tests/test-imagex-capture_and_apply` | Shared directory cases across None/XPRESS/LZX, split+reference apply, join, export; None pipable file/pipe paths; corrupt-stream recovery; capture exclusions; reparse fixups; source-list overlays | Built `tree-cmp`, symlink/hardlink-capable filesystem; exclusion fixtures below; does not cover full LZMS/solid behavior |
| `tests/test-imagex-update_and_extract` | Update add/delete/rename, invalid commands/options, force/recursive, rebuild, UTF-16LE with/without BOM, UTF-8 BOM; extract/stdout/path lists/glob/nullglob/case behavior | Bash and built CLI; CLI grammar tests need separate library-contract tests |
| `tests/test-imagex-mount` | Read-only compression matrix; read-write changes, discard/commit/check; tar create/delete/recreate using all shared cases; committed image apply | FUSE enabled, readable/writable `/dev/fuse`, `fusermount3`, tar; inaccessible FUSE exits 77 |
| `tests/test-imagex-ntfs` | Direct NTFS-image capture/apply; streams, DOS names, object IDs, security descriptors, links and ordinary trees | ntfs-3g enabled; `mkntfs`, `ntfs-3g`, `fusermount`, `mountpoint`, `setfattr`; mount privileges; unable initial unprivileged mount exits 77; failed tests can leave mounts |
| `tests/win32-test-imagex-capture_and_apply.bat` | Native Windows capture/apply twice, including existing destination; independent DISM apply and metadata tree comparison | Administrator, NTFS, DISM, `win32-tree-cmp.exe`, `set_reparse_point.exe`, CLI in working directory; explicit DISM skips must remain visible |
| `tests/win32-test-imagex-capture_and_apply.sh` | Runs Windows build tool, builds two helpers, invokes batch suite | Windows Bash/MinGW environment and Windows command execution; not a Linux Windows-emulation gate |
| `tests/tree-cmp.c` | Names, contents, sizes, modes, access times, hardlink equivalence; NTFS mode adds NTFS metadata | Separate oracle executable; compare link partitions rather than literal cross-tree inode numbers |
| `tests/win32-tree-cmp.c` | Native streams, short names, security and filesystem metadata comparator | Windows native APIs and privileges; preserve upstream comparator separately from replacement |
| `tests/set_reparse_point.c` | Creates arbitrary Windows reparse payloads | Fixture helper, not standalone test suite |
| `tests/wlfuzz.c` | Random operation sequences: create/add-empty/delete-image/delete-WIM/verify/unchanged overwrite/export/apply-recapture/split/compression level; Windows WIMBoot operation | `--enable-test-support`; public and internal test APIs; time argument in seconds; zero/absent means endless; current seed uses time and PID |
| `tools/libFuzzer/{compress,decompress,encoding,wim,xml_windows,xmlproc}/fuzz.c` | Six independent fuzz targets with shipped corpora; compression roundtrip, malformed decoder input, encoding, WIM lifecycle, Windows XML discovery, XML parse/write stability | Clang libFuzzer; `fuzz.sh` reconfigures in place; allocator fault injection in `fault-injection.c`; internal helpers require test-support build |
| `tools/libFuzzer/test-one-input.c` | Single-input replay driver | Used by `fuzz.sh --input`; not a suite |
| `tools/test-examples.sh` | Example clients in C/C++, Linux execution and Windows cross-compilation | Hardcoded `$HOME/data/test.wim`, `$HOME/data/testdata`, Windows library locations; customize a disposable copy |
| `tools/msvc-test-examples.bat` | MSVC example-client build checks | Windows/MSVC setup; complement with runtime client tests against replacement |

## Exact named scenario ledger

The following labels and line numbers are mechanically transcribed from this revision. Parameter loops expand these into multiple executions; labels are not a coverage percentage. Commented-out Windows cases are excluded and must be recorded as missing upstream coverage. Each assertion beneath a label must receive a case ID during porting; labels alone do not identify every assertion.

### `tests/test-imagex`

| Source line | Scenario |
| --- | --- |
| 36 | Testing capture and application of $comp_type-compressed WIM |
| 96 | Testing capture of WIM with default name and description |
| 105 | Testing capture of WIM with default boot flag |
| 111 | Testing changing image bootable flag |
| 118 | Testing changing image bootable flag |
| 125 | Testing changing image bootable flag to invalid image (this should generate errors) |
| 148 | Testing capture of WIM with name and description |
| 151 | Testing capture of WIM with non-ASCII name and description |
| 154 | Testing printing WIM lookup table |
| 158 | Testing printing WIM header |
| 162 | Testing printing WIM XML info |
| 166 | Testing extracting WIM XML info |
| 170 | Testing printing WIM metadata |
| 176 | Testing capture of bootable WIM |
| 187 | Testing capture of WIM with integrity table |
| 204 | Testing appending WIM image |
| 213 | Testing appending WIM image with existing name (this should generate errors) |
| 217 | Testing appending WIM image with new name |
| 221 | Testing appending WIM image with integrity check |
| 228 | Testing appending WIM image with no integrity check |
| 239 | Testing deleting first WIM image |
| 246 | Testing deleting last WIM image |
| 253 | Testing deleting invalid WIM image (this should generate errors) |
| 260 | Testing deleting all WIM images |
| 267 | Testing appending directory to empty WIM and making it bootable |
| 277 | Testing appending non-directory (should generate errors) |
| 281 | Testing appending non-existent file (should generate errors) |
| 286 | Testing appending directory containing unreadable file (should generate errors) |
| 298 | Testing application of multiple images |
| 322 | Testing application of single image containing identical files |
| 342 | Creating random files to test WIM splitting on |
| 352 | Splitting WIM into 1 MiB chunks |
| 356 | Verifying the split WIMs (some errors expected) |
| 374 | Joining the split WIMs and applying the result |
| 393 | Testing export of single image to new WIM |
| 406 | Testing export of single image to existing WIM |
| 413 | Testing export of single image to existing WIM using wrong compression type |
| 418 | Testing export of multiple images to new WIM |
| 428 | Testing export of multiple images to existing WIM |
| 432 | Testing export of multiple images to existing WIM with --boot |
| 442 | Testing export of multiple images to existing WIM with --boot, but no bootable image (errors expected) |
| 479 | Testing exporting then applying an image (\"$cflag1\" => \"$cflag2\") |

### `tests/common_tests.sh`

| Source line | Scenario |
| --- | --- |
| 2 | nothing |
| 5 | a single file |
| 8 | a single directory |
| 11 | subdirectory with file |
| 14 | empty file |
| 17 | two empty files |
| 20 | hard link in same directory |
| 23 | hard link between empty files |
| 26 | relative symbolic link |
| 29 | absolute symbolic link |
| 32 | large file |
| 35 | many nested directories |
| 38 | identical files and symlinks in subdirectory |
| 44 | hard link group and identical files not hard linked |
| 51 | C source code of wimlib |
| 54 | tons of random stuff |

### `tests/test-imagex-update_and_extract`

| Source line | Scenario |
| --- | --- |
| 45 | Testing deleting nonexisting file from WIM image, without --force (errors expected) |
| 50 | Testing deleting nonexisting file from WIM image, with --force |
| 55 | Testing deleting root directory from WIM image, without --recursive (errors expected) |
| 60 | Testing deleting root directory from WIM image, with --recursive |
| 65 | Testing update command with invalid option (errors expected) |
| 70 | Testing update command with too many arguments (errors expected) |
| 75 | Testing invalid update command (errors expected) |
| 80 | Testing update command file with comments and empty lines |
| 92 | Testing update with --rebuild |
| 111 | Testing renaming file in WIM image |
| 123 | Testing UTF-16LE-NOBOM command update file |
| 130 | Testing UTF-16LE-BOM command update file |
| 137 | Testing UTF-8-BOM command update file |
| 143 | Testing adding, then renaming file in WIM image in one command |
| 152 | Testing adding additional file to WIM image |
| 164 | Testing extracting file from WIM image |
| 171 | Testing extracting file from WIM image to stdout |
| 178 | Testing adding directories and files to WIM image |
| 203 | Testing adding files to WIM image |
| 218 | Testing adding file with space in it |
| 228 | Testing path list extract |
| 248 | Testing path list extract (stdin) |
| 258 | Testing path list extract (w/ wildcard) |
| 272 | Testing path list extract (no wildcard, no match; error expected) |
| 279 | Testing path list extract (wildcard, no match; error expected) |
| 281 | Testing path list extract (wildcard, no match, nullglob; no error expected) |
| 284 | Testing path list extract (w/ wildcard) |
| 294 | Testing path list extract (subdir files) |
| 310 | Testing case insensitivity |

### `tests/test-imagex-ntfs`

| Source line | Scenario |
| --- | --- |
| 124 | Empty NTFS volume |
| 127 | a single file |
| 130 | a single directory |
| 133 | subdirectory with file |
| 136 | empty file |
| 139 | two empty files |
| 142 | hard link in same directory |
| 145 | hard link between empty files |
| 148 | relative symbolic link |
| 151 | absolute symbolic link |
| 154 | large file |
| 157 | file with DOS name |
| 160 | many nested directories |
| 163 | identical files and symlinks in subdirectory |
| 169 | hard link group and identical files not hard linked |
| 176 | file with named data stream |
| 180 | file with multiple named data streams |
| 187 | file with multiple named data streams with same contents |
| 194 | file with named data streams with same contents as other file |
| 199 | file with empty named data stream and non-empty unnamed data stream |
| 203 | file with empty named data stream and empty unnamed data stream |
| 207 | file with named data stream with hardlink |
| 212 | C source code of wimlib |
| 215 | file with security descriptor |
| 219 | file with object ID |
| 225 | files with different security descriptors |
| 231 | files with different security descriptors and some with the same security descriptor |
| 241 | tons of random stuff |

### `tests/win32-test-imagex-capture_and_apply.bat`

| Source line | Scenario |
| --- | --- |
| 25 | empty directory |
| 28 | single file |
| 32 | different files |
| 37 | identical files |
| 42 | hard linked file |
| 47 | hard linked file, with other identical files |
| 53 | empty file |
| 57 | hard linked empty file |
| 63 | various hard linked, identical, different, and empty files |
| 77 | multiple subdirectories, some empty, some not |
| 95 | directory with custom security descriptor (inheritence enabled) |
| 100 | directory with custom security descriptor (inheritence disabled) |
| 112 | file with custom integrity level |
| 118 | relative symlink |
| 122 | absolute symlink, with drive letter |
| 126 | absolute symlink, without drive letter |
| 130 | relative symlink, with file target |
| 135 | relative symlink, with directory target |
| 140 | junction |
| 145 | symlinks, junctions, files, subdirectories, etc. |
| 164 | reparse point that is neither a symlink nor a junction |
| 169 | reparse point with named data streams |
| 176 | reparse point with unnamed data stream |
| 181 | reparse point with unnamed data stream and named data streams |
| 188 | empty reparse point |
| 193 | empty reparse point with unnamed data stream |
| 198 | empty reparse point with unnamed data stream and named data streams |
| 205 | maximum length reparse point |
| 210 | directory reparse point that is neither a symlink nor a junction |
| 215 | directory reparse point with named data streams |
| 222 | compressed file |
| 227 | compressed directory |
| 232 | compressed directory with files in it |
| 243 | compressed directory with some uncompressed files in it |
| 251 | file with alternate data stream |
| 256 | file with multiple alternate data streams |
| 264 | file with multiple alternate data streams, with hard link |
| 273 | files with multiple alternate data streams, some identical, with hard link |
| 284 | file with empty alternate data stream |
| 289 | directory with empty alternate data stream |
| 294 | root directory with alternate data stream |
| 298 | root directory with empty alternate data stream |
| 302 | subdirectory with alternate data streams |
| 309 | subdirectories and files with alternate data streams |
| 322 | symbolic link and hard link, to file with alternate data streams |
| 329 | compressed file with alternate data streams |
| 336 | hidden file |
| 341 | hidden system file |
| 346 | hidden, readonly, system file |
| 351 | hidden directory |
| 356 | hidden system directory |
| 361 | hidden, readonly, system directory |
| 366 | readonly directory with named data stream |
| 372 | encrypted file |
| 377 | identical encrypted files |
| 387 | encrypted directory |
| 392 | encrypted directory with encrypted file in it |
| 399 | encrypted directory with unencrypted file in it |
| 415 | unencrypted file in encrypted directory in compressed directory |
| 424 | encrypted directory with alternate data streams |
| 431 | hardlinked, encrypted file with alternate data streams |
| 443 | file with object ID |
| 448 | directory with object ID |
| 453 | sparse files |
| 466 | sparse and compressed files |

## Fixtures

All eight `tests/wims/*.wim` files are required: `corrupted_file_1.wim` (SHA-1 mismatch), `corrupted_file_2.wim` (decompression failure), `cyclic.wim` (cyclic directory), `duplicate_names.wim` (duplicate sibling name), `dotdot.wim` (parent traversal), `longpaths.wim` (Windows long path), `empty_dacl.wim` (valid empty DACL), `linux_xattrs_old.wim` (legacy xattr tag). The descriptions are grounded in `tests/wims/README`. Merely shipping a fixture does not prove a script exercises it: capture/apply explicitly uses the two corruption fixtures; add direct regressions for every other fixture. Never weaken traversal checks to imitate an unsafe third-party extractor.

The thirteen exclusion files are `anchored_exception_in_excluded_dir`, `anchored_prefix_match`, `anchored_simple`, `case_insensitive`, `case_sensitive`, `inner_star`, `multiple_stars`, `prefix_match`, `question_mark`, `recursive_match`, `simple_exception`, `suffix_match`, and `wildcard_exception`. Their first section is capture configuration; after `@@@`, the tree specification marks excluded entries with `- `. Retain each file unchanged and parameterize the port by fixture name. The two security descriptors are supplied as binary and base64 pairs; preserve both representations and verify decoded bytes agree.

## Running the oracle

Run these commands only in a disposable **copy** of the pinned upstream source. `configure`, fuzz tooling, mount suites and Windows suites mutate their working trees. Do not reconfigure a source tree another worker is building. Build prerequisites include a C toolchain, make, Bash, autoconf, automake, libtool and pkg-config; optional platform libraries and tools are listed above. For a Git checkout without generated `configure`, run `./bootstrap` first (it runs `autoreconf -i`); repeat this preparation in each separate source copy. Capture the actual configuration output; disabled features are missing gates.

```sh
# Portable Unix oracle, from disposable upstream root
./bootstrap # Git checkouts: invokes autoreconf -i
./configure --without-fuse --without-ntfs-3g --enable-test-support
make -j4
make check TESTS='tests/test-imagex tests/test-imagex-capture_and_apply tests/test-imagex-update_and_extract'
make tests/wlfuzz
./tests/wlfuzz 60

# Separate feature-enabled Linux oracle copy
./configure --enable-test-support
make -j4
make check TESTS='tests/test-imagex-mount tests/test-imagex-ntfs'

# Separate Clang oracle copy; each invocation can rebuild/reconfigure
./tools/libFuzzer/fuzz.sh --asan --ubsan --time=60 decompress
./tools/libFuzzer/fuzz.sh --asan --ubsan --time=60 compress
./tools/libFuzzer/fuzz.sh --asan --ubsan --time=60 wim
./tools/libFuzzer/fuzz.sh --asan --ubsan --time=60 encoding
./tools/libFuzzer/fuzz.sh --asan --ubsan --time=60 xmlproc
./tools/libFuzzer/fuzz.sh --asan --ubsan --time=60 xml_windows
```

`tests/test_utils.sh` calls `../../wimlib-imagex` from suite-specific temporary directories. Putting a replacement on `PATH` cannot redirect it. In a disposable harness, supply that exact path with the chosen executable or change only `wimlib_imagex()` to use an absolute `WIMLIB_IMAGEX` variable and retain a reviewed patch. Build `tree-cmp` before manually invoking scripts. Do not run two instances of the same suite in one build directory: temporary names are fixed. Different suite names are isolated, but FUSE/NTFS resource cleanup still requires careful serialization.

On Windows, build the CLI and helpers, stage them with the batch script in a disposable NTFS directory and run the batch script elevated. Its first application and overwrite application both need success. DISM tests have known explicit exclusions (for example empty hardlinks and preservation of some encrypted-file short names). Preserve these reasons and add replacement-vs-upstream checks for excluded metadata; skipping DISM is not permission to skip Rust compatibility. Some security and encrypted-root cases are commented out upstream, so they require new tests.

## Parallel TDD migration contract

Before coding, the harness owner creates a checked-in case manifest with stable IDs, source revision/path/line, input hashes, parameters, oracle command, expected return status/API code, expected state after failure, comparison rules, platform, owner, and dependencies. Record each loop expansion as a case. Run the pinned original first and save an immutable baseline. Distinguish pass, failure, environment skip, feature-disabled and not-run; only pass is gate evidence.

| Parallel owner | Red tests to implement first | Required shared contract / handoff |
| --- | --- | --- |
| Format and metadata | All malformed WIM fixtures, roundtrip metadata/header/XML, longpaths and legacy xattrs; truncated tables and checked offsets | Immutable parsed metadata/stream model, explicit format errors, fixture snapshots |
| Codecs | Shipped compression/decompression corpora, each valid block decoded by both engines, all malformed blocks, boundary sizes and level settings | Byte-oriented codec API with caller capacities; XPRESS/LZX/LZMS vectors; no filesystem dependency |
| Capture and filesystem scan | Shared directory cases, exclusions, source lists/overlays and reparse fixups | Normalized metadata/stream model; deterministic virtual scanner and platform adapters |
| Writer, resource and image lifecycle | Basic append/delete/export/split/join/integrity, unchanged overwrite, pipable file and pipe capture | Writer consumes shared model, injectable seek/write/flush failures, cross-reader validation |
| Extraction and update | Update/extract ledger, corruption recovery, stream ordering, hardlinks, existing targets, failures | Validated model plus filesystem operations trait; negative tests observe destination state |
| Windows and direct NTFS | Windows ledger, NTFS suite, descriptor fixtures, WIMBoot fuzz operation | Shared stream/metadata model; native API adapter; elevated disposable test runner |
| Mount | Existing mount suite and shared cases through tar cycles, discard/commit/check | Image/update interfaces settled; mount lifecycle and flush/commit contract |
| C ABI and CLI | Original example clients, public API harness, unchanged upstream CLI scripts | Generated symbol/type/constants manifest, stable error/progress/ownership contract |

For every case: (1) prove its assertion fails against a deliberate stub or broken behavior; (2) run original to establish result; (3) implement the Rust path; (4) run Rust unit and differential test; (5) run downstream consumer tests before integration. Agents can write failing tests against agreed trait signatures before dependencies exist; the dependency owner supplies mocks or fixture readers, never success-returning production placeholders. Shared contract changes require consumer test updates and review. Each owner works in an isolated checkout; fixture baselines and schema are immutable shared artifacts. Merge format/model contracts first, then codecs and metadata parsing; capture/extraction/writer follow in parallel; mounting, platform integration and complete ABI acceptance follow their dependencies.

Rust tests should use descriptive behavior names, `Result`-based failures and bounded test inputs. Keep unsafe code confined to the ABI/platform adapter and test it separately. Ordinary `cargo test --locked` must not silently elevate or mount; explicit platform jobs run the required ignored tests and report skips. Upstream case ports are necessary regression coverage, not sufficient specification coverage.

## Upstream harness defects to correct in the migration harness

`test-imagex-capture_and_apply:19–29` and `test-imagex-ntfs:102–109` call `error` after a failed tree comparison only inside the branch checking executable `/usr/bin/tree`. Without that optional diagnostic utility, a metadata comparison failure can escape the intended fatal path. Preserve the comparator, but make comparison failure unconditional in a reviewed harness patch, and test the harness with a deliberately mismatching tree while `tree` is absent. This is a harness-strengthening correction, not a change to the format oracle. Do not accept an upstream green log without examining such assertion paths. Add `set -o pipefail` to the reviewed Bash harness where producer failures in streaming pipelines would otherwise be hidden; separately test producer and consumer exit codes.

`tools/libFuzzer/decompress/fuzz.c:9–21` computes requested output as three times the original input size, but constructs its decoder with maximum block size equal to input size minus the codec-selector byte. `src/decompress.c:97–98` returns `-2` whenever requested output exceeds that capacity, before dispatching to the codec. Thus this revision's shipped decompression fuzz entry point cannot establish decoder coverage. Reuse the corpus with a reviewed harness that creates adequate output capacity, strips its selector, and demonstrably invokes the decoder. Keep oversized-capacity rejection as its own API test; do not conflate it with malformed-block rejection.

## Differential oracle design

Use four directions for every supported WIM variant: original writes/original reads (baseline), original writes/Rust reads, Rust writes/original reads, Rust writes/Rust reads. Add independent Windows DISM/WIMGAPI readers where format interoperability is claimed. Comparing only Rust roundtrips permits mutually consistent writer/reader bugs. Compression output need not be byte-identical unless an API explicitly promises deterministic bytes; decompressed bytes, capacity behavior, valid format and return semantics must agree.

A normalized semantic snapshot contains image names/descriptions/boot index/properties; paths and exact filename code units; timestamps and attributes; security descriptor bytes/meaning; reparse tags/payloads; all named and unnamed streams with lengths and digests; hardlink equivalence classes; short names/object IDs/Unix metadata; split references; compression and integrity metadata. Normalization may remove only documented nondeterminism (GUIDs, wall clock, physical resource layout), with a named rationale per field. Preserve duplicate-name rejection and case-sensitive/insensitive differences. Compare observable CLI stdout/stderr and exit status separately from library numeric errors; CLI nonzero status alone does not establish API error-code parity.

For C ABI tests, compile the same C and C++ client sources with the original public header and run against each library in separate processes. Compare symbol names, calling conventions, exported versions, structure size/alignment/field offsets, enum/flag values, `tchar` widths, callbacks, allocator hooks, ownership and cleanup, error values, and API side effects. Test callback cancellation/abort and resource lifetimes, null/invalid inputs documented by each API, custom allocator failures and all public functions not touched by shell suites. Test internal support functions only in a dedicated compatibility harness; they are not automatically public replacement obligations.

Port `wlfuzz` operation sequences to a deterministic Rust property runner. Save the seed and full operation trace, reduce failures, and replay across original and Rust in separate processes. The original has no seed command-line switch; a minimal reviewed oracle patch can expose its seed, or capture generated WIMs and sequences externally. Its seconds argument is a time limit, not a seed. Preserve WIMBoot and metadata distinctions; serialized Rust allocation-failure injection must not corrupt global state across unrelated tests. Share libFuzzer corpora between engines only after preserving each harness's leading-byte encoding and fault-injection framing.

Inject read/seek/write/flush/allocation errors, cancellation, corrupt compressed resources, absent split parts and invalid state transitions. Assert failure return codes, file validity or documented recovery state, no unintended image mutations, handle cleanup and no paths outside extraction root. Use sanitizer-enabled C oracle runs, Rust fuzzing and bounded memory/time budgets. Retain failing minimized inputs and hashes permanently.

## Drop-in release gates

1. Every case in the upstream manifest and all public API contracts pass on their applicable platforms; skips and feature-disabled configurations block claims for those features.
2. Original C/C++ examples compile and run unchanged against the Rust library. ABI/export inspection passes on supported Linux, Windows x86/x64 and any other explicitly claimed architectures, including the library naming/loading behavior.
3. Every codec, compression level/chunk-size mode, LZMS/solid, split/pipable/delta/reference-resource and integrity behavior has cross-reader/writer coverage. The three-codec shell loops omit LZMS; add that matrix explicitly.
4. Native Windows metadata, privileges, long paths, EFS, ADS, reparse points, WIMBoot/WOF-related behavior and repeated apply pass on appropriate Windows versions and NTFS volumes. Direct NTFS and FUSE mount gates pass separately; Linux tests cannot establish them.
5. Callback/progress, abort behavior, allocator failure and recovery/error semantics match the documented public contract, with tests supplementing shell coverage.
6. Independent DISM interoperability and representative real multi-image media pass where promised. A successful WIM write, ISO write or Linux extraction cannot prove Windows servicing/installation correctness.
7. Release artifacts record Rust/C source revisions, feature flags, tool versions, test IDs, fixture hashes, raw logs, platform and unsupported/untested claims. Library core and codecs must execute Rust-native implementations, with no runtime fallback to upstream wimlib. Performance and memory limits are measured on agreed workloads; publish regressions rather than treating unspecified performance as passing.

These gates specify the work remaining for a replacement. This document does not certify a replacement exists, and baseline suite execution has not been performed as part of this source inventory.

## Native Rust honggfuzz harnesses

The standalone [fuzz package](../fuzz/README.md) exercises WIM header/metadata,
archive resources, CAB member streaming, all native decoding algorithms, and
XPRESS/LZX/LZMS encoder round trips with context reuse. Corpus seeds come from
checked-in interoperability/CVE fixtures and valid native encoder output.
Malformed-block targets configure an adequate decoder maximum so they reach
the codecs rather than only testing oversized-output rejection.

This adds bounded native parser/codec coverage, not original-C differential
fuzzing, C ABI pointer validation, servicing, or platform installation evidence.
Use the documented replay command and preserve failing inputs as regressions.

The initial Linux smoke validation on 2026-10-03 completed 1,001 instrumented
iterations for each of the four targets, with no crashes or timeouts. This
result does not replace sustained fuzzing or independent format oracles.
