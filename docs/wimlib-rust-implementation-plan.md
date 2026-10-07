# Native Rust wimlib replacement: implementation and parallel TDD plan

## Target and evidence boundary

This is a plan for implementing the complete library represented by `/tmp/wimlib`, including a compatible C interface and a Rust interface. It is not an implementation or a claim of compatibility. The [API reference](wimlib-api-reference.md) inventories all **72 exported functions**, layouts, flags, and callbacks; the [test strategy](wimlib-test-strategy.md) supplies named original scenarios and fixture/fuzz gates. This document describes how to implement and prove those contracts. All upstream paths below are relative to `/tmp/wimlib`. The inspected baseline is wimlib 1.14.5, commit `cd5e231c348c255ae5088873b5a66ee0eb96fa07`.

The replacement must read and write ordinary WIM, split WIM, solid/LZMS ESD-style WIM, and wimlib's pipable extension; capture, apply, extract, update, export, split, join, verify, mount, and unmount; preserve supported filesystem metadata; expose standalone compression APIs; and implement every public symbol, error, flag, callback, allocation rule, and supported platform configuration in `include/wimlib.h`. Unsupported operations must match the upstream configuration's documented failure rather than silently lose data. Merely linking, passing Linux round trips, or decoding a Windows installation WIM does not establish drop-in parity.

Baseline the exact source tree before implementation. Record git revision if present, version from `tools/get-version-number.sh`, source SHA-256 manifest, configuration options, compiler, target ABI, enabled optional dependencies, public header hash, exported symbols, and fixture hashes. `Makefile.am` uses libtool version information `42:0:27`, giving the GNU/Linux ABI major 15 (`libwim.so.15`); additionally verify the installed SONAME from an actual baseline build. Record Windows DLL exports and calling conventions separately. Freeze a source copy and do not alter upstream tests in place.

Rust-native means production WIM, metadata, compression, and orchestration algorithms do not call or embed the original C implementation. C compilation is allowed for compatibility probes, existing tests, and the differential oracle. OS system calls are permitted. Optional integrations with FUSE and filesystem services need an explicit dependency policy; a replacement that calls libntfs-3g for NTFS parsing does not meet the native goal. The default requirement is a native NTFS backend; changing that to a system adapter requires an explicit scope decision and cannot be described as complete native parity.

## Current allocator scope

The Rust API and `no_std + alloc` format core use Rust's global allocator.
The `wim-memory` crate and `wimlib_set_memory_allocator` registration are removed.
Complete upstream allocator-hook and recoverable OOM compatibility is no longer
a goal. Fallible collection reservation remains where supported; ordinary Box
and Arc allocation follows the global allocator policy. C-owned output buffers
retain fixed host-C allocation and matching free contracts. The original parity
requirements below are historical where they conflict with this scope.

## Compatibility acceptance ledger

Create a machine-readable ledger with one entry per public symbol and flag combination: upstream declaration/source, required feature/platform, Rust owner, observable semantics, relevant original test, additional test, status, and evidence artifact. Separate API availability from behavior and ABI. Include negative argument cases, global initialization, allocator hooks, diagnostics, progress cancellation, borrowed-string lifetimes, image numbering, handle lifetime, and configuration-specific errors.

The final gate requires all entries either passing on the corresponding supported configuration or marked inapplicable with the matching upstream configuration evidence. A temporary stub, an unsupported platform, or a skipped required test remains incomplete. Default build and all optional supported configurations must be represented; a minimal upstream build is not the whole target.

## Proposed workspace and ownership

The current implementation uses the root workspace: `crates/wim`,
`crates/wim-format` and `crates/wim-types`, with explicit
WIM licenses. `wim` separates its public Rust API from `wim::ffi` compatibility
exports and keeps low-level implementation under `src/engine/`. The table below
retains the proposed ownership boundaries; its additional core, I/O, filesystem,
mount, CLI, and oracle crates have not all been split into separate packages.

| Crate/module | Responsibility | Upstream starting points |
| --- | --- | --- |
| `wim-types` | Checked identifiers, names, flags, times, hashes, errors, events | `include/wimlib.h`, `encoding.c`, `timestamp.c`, `error.c` |
| `ms-compress` | XPRESS/LZX/LZMS encode/decode, match finders, standalone contexts | `*_compress.c`, `*_decompress.c`, `compress_common.c`, `decompress_common.c`, `divsufsort.c`, `lcpit_matchfinder.c` |
| `wim-format` | Header/resource/blob/metadata/XML/integrity encoders and parsers | `header.c`, `resource.c`, `blob_table.c`, `metadata_resource.c`, `tagged_items.c`, `xml.c`, `integrity.c` |
| `wim-core` | Handle/image state, references, iteration, update/export/delete | `wim.c`, `dentry.c`, `inode.c`, `reference.c`, `iterate_dir.c`, `update_image.c`, `export_image.c`, `delete_image.c` |
| `wim-io` | Random/sequential I/O, chunk reads, writer transactions, solid/pipable/split | `file_io.c`, `write.c`, `solid.c`, `split.c`, `join.c`, `compress_parallel.c` |
| `wim-fs` | Capture/apply abstraction and Unix/Windows backends | `scan.c`, `extract.c`, `unix_*`, `win32_*`, `security.c`, `reparse.c`, `inode_fixup.c` |
| `wim-ntfs` | Native NTFS volume capture/apply | `ntfs-3g_capture.c`, `ntfs-3g_apply.c`, `include/wimlib/ntfs_3g.h` |
| `wim-mount` | FUSE view, writable overlay, commit/unmount protocol | `mount_image.c` |
| `wim` | WIM engine, owned Rust API, compatible C exports, selected C layouts, host-C output ownership, panic containment | `include/wimlib.h`, `progress.c`, `util.c` |
| `wim-cli` | Existing imagex command behavior and diagnostics | `programs/imagex.c`, `imagex-win32.c`, `wgetopt.c` |
| `wim-oracle` | Test-only C probes, subprocess baseline, fixtures, ABI checks | `tests/`, `src/test_support.c` |

Keep format parsing independent of filesystem backends and FFI. Avoid a single crate with cyclic platform dependencies. Prefer checked arena IDs for inodes/dentries to pointer-linked mutable graphs; keep directory entries distinct from inode identity and data-blob identity. Identical content does not imply a hard link.

## Interfaces to freeze before parallel work

These are proposed internal contracts, not existing APIs. Owners first merge interface definitions, error mappings, mocks, and tests; consumers then work against them.

| Interface | Contract and invariants |
| --- | --- |
| `ReadAt::read_exact_at(offset, out)` | Thread-safe shared immutable source; checked offset arithmetic; exact read or typed error. Never shared mutable seek state. |
| `SequentialSource::read_exact(out)` | No seek assumption; explicit EOF and short-read mapping; pipable reader state machine owns stream position. |
| `OutputSink` | Write-all, seek capability, flush, durability capability, ownership policy; descriptor APIs do not accidentally close caller-owned descriptors. |
| `Codec::decode(input, output)` | Decoder context per worker, bounded output, exact upstream acceptance/error mapping, no read outside either buffer. |
| `Compressor::compress(input, capacity)` | Compression type/block limit/level validated at context creation; result distinguishes compressed bytes from upstream incompressible/failure sentinel semantics. |
| `ResourceReader::read_range(resource, range, consumer)` | Handles raw, compressed, solid, and pipable resources; verified bounds; shared decompressed-chunk cache optional and bounded. |
| `BlobStore::resolve(hash)` | Content SHA-1 lookup with explicit missing/corrupt/ambiguous source errors; reference counts separate from stream references. |
| `ImageGraph` | Root/dentry/inode/security/stream arenas with checked IDs; preserves unknown supported tagged payloads and UTF-16 names. |
| `CaptureBackend::scan(options, events)` | Returns metadata and lazily readable blob sources; native file identity and change detection retained; warning policy explicit. |
| `ApplyBackend::apply(plan, resources, events)` | Declares capabilities before mutation; directory/stream/link/security/finalization phases; honors strict and permissive flags. |
| `ProgressSink::emit(event)` | Synchronous observable callback ordering at coordinator; continue/abort mapped to original numeric statuses; no borrowed data retained past callback. |
| `WritePlan` / `UpdateTransaction` | Immutable plan and staged mutation; actual upstream rollback guarantees recorded per operation; do not promise stronger semantics across filesystem failures. |

Use `Result<T, WimError>` internally with a stable explicit translation to original integer error codes at FFI. Retain contextual Rust errors without changing C return values. Public methods should borrow where possible; use `Arc` only for shared immutable resource ownership that outlives handles or workers. Document whether a handle is `Send`/`Sync`; do not assert concurrent operations on one C handle are valid without upstream evidence.

Proposed core signatures (final types are to be agreed in P1):

```rust,ignore
struct WimName(Vec<u16>); // Archive names retain code units, including non-UTF-8 names.
struct DentryId(u32);
struct InodeId(u32);
struct StreamId(u32);
struct BlobHash([u8; 20]);
enum BlobSource {
    Archive { source: Arc<dyn ReadAt>, resource: ResourceDescriptor, range: Range<u64> },
    Captured { source: Arc<dyn CapturedFile>, expected_identity: FileIdentity },
    Spool { source: Arc<dyn ReadAt>, range: Range<u64> },
}
trait ReadAt: Send + Sync {
    fn len(&self) -> Result<u64, WimError>;
    fn read_exact_at(&self, offset: u64, output: &mut [u8]) -> Result<(), WimError>;
}
trait BlockDecoder {
    fn decode(&mut self, input: &[u8], output: &mut [u8]) -> Result<(), WimError>;
}
trait ProgressSink {
    fn emit(&mut self, event: ProgressEvent<'_>) -> Result<(), WimError>;
}
```

These signatures are design sketches, not compilable current code. Graph IDs are scoped to their owning image arena and cannot be resolved through another image; deletion invalidates or generation-tags IDs. Public iterators expose borrowed views bounded by the graph borrow, while FFI callback projections last only until callback return. Store source references in `BlobSource`, not handle-relative raw pointers, so export/reference lifetimes are provable. Captured source identities also include change-detection data required by upstream behavior; spool ownership deletes only owned temporary files.

## Format implementation sequence and algorithms

### Header and resources

Implement explicit little-endian field readers/writers, not transmutation of Rust structs. `include/wimlib/header.h` specifies a 208-byte header, ordinary version `0x10d00`, solid version `0xe00`, ordinary `MSWIM` and pipable `WLPWM` magic, 1-based split part fields, and image count/boot index. Preserve the upstream distinction between zero images (accepted by wimlib) and WIMGAPI's expectations. Match header validation in `src/header.c`, including allowed version/flag/chunk-size combinations and the baseline's image-count limit (currently 65535).

Resource headers are 24 bytes: 7-byte stored size, 1-byte flags, 8-byte offset, 8-byte uncompressed size (`include/wimlib/resource.h`). Parse the 56-bit size safely. Validate additions, source length, offsets, flags, chunk tables, decompressed lengths, and partial final chunks before allocating. Implement raw reads first, then ordinary chunked reads, solid resource blob ranges, and pipable framing. Solid resources are multiple blobs compressed together; the main lookup-table entry uses the special uncompressed-size marker `0x100000000`. Derive exact chunk-table layouts and special entries from `resource.c`, `solid.c`, and `blob_table.c` before coding.

### Metadata and image model

Implement security table and directory record parsing from `metadata_resource.c`, `dentry.c`, `inode.c`, `security.c`, and their internal headers. Add stream records, short names, hardlink grouping, timestamps, attributes, reparse data, object IDs, UNIX metadata, extended attributes, and tagged items. Reject malformed graph structure without recursion overflow. Use explicit iterative traversal and depth/memory limits consistent with upstream where externally observable. Preserve UTF-16 code units rather than forcing all names into Rust UTF-8 strings; Windows names and malformed-but-supported encodings require baseline tests.

Encode metadata only after original metadata fixtures decode into canonical manifests. A manifest records inode equivalence classes, individual directory names, security descriptor bytes, streams with hashes, times, flags, reparse bytes, and tagged payloads. Canonicalize only ordering known to be unobservable; do not erase duplicate-name, hardlink, timestamp, or case-sensitive distinctions to make tests pass.

### XML, SHA-1, integrity, and ancillary formats

Implement image properties and XML round trips (`xml.c`, `xmlproc.c`, `xml_windows.c`), retaining unknown properties and matching property-path updates/removals, image renumbering, names, and encoding. Do not use a generic serializer that changes required semantics. Implement SHA-1 for WIM content identification and integrity checking; it is a format requirement. Test integrity coverage against `integrity.c` independently from content SHA-1. Implement registry reading and reparse transformations from `registry.c`, `reparse.c`, and `wimboot.c`; these are required platform behavior, not optional metadata polish.

### Compression

Implement and test decoders independently before writers: XPRESS Huffman, LZX block/Huffman/window handling and E8 transform, LZMS range/Huffman/state/history handling and transformations. Extract edge behavior from the corresponding original codec files and constants, rather than assuming unrelated crates implement the exact WIM dialect. Add scalar implementations first; CPU acceleration comes after equivalence tests. Compression work includes match finding, level mapping, memory requirements, maximum block size, incompressible return semantics, and allocation failure.

Output need not use identical compressed bytes unless an observable API or test requires them; it must decode under the original library and relevant Windows readers and preserve semantic results. Decode original output and original-decode Rust output for every codec, chunk size, level, and boundary case. Round-tripping only Rust encode/decode can hide shared defects. Build explicit LZMS and solid tests: the normal upstream shell compression loop covers None, XPRESS, and LZX, not complete LZMS parity.

Codec packages can split further without sharing mutable work files:

| Subtask | Source authority | Boundary tests |
| --- | --- | --- |
| Common bit I/O and canonical Huffman construction | `include/wimlib/decompress_common.h`, `compress_common.h`, `src/decompress_common.c`, `compress_common.c` | Truncated words, invalid code lengths, oversubscribed tables, end-of-buffer behavior |
| XPRESS symbol/match coding | `include/wimlib/xpress_constants.h`, `src/xpress_compress.c`, `xpress_decompress.c` | Literal-only, repeated offsets, long-length extensions, output limit |
| LZX trees/window/transform | `include/wimlib/lzx_constants.h`, `lzx_common.h`, `src/lzx_common.c`, `lzx_compress.c`, `lzx_decompress.c` | All block types, repeated-offset rotation, window crossings, E8 boundary transformation |
| LZMS state/coders/transforms | `include/wimlib/lzms_constants.h`, `lzms_common.h`, `src/lzms_common.c`, `lzms_compress.c`, `lzms_decompress.c` | Probability/history resets, literal/match streams, offset/length extremes, transform boundaries |
| Fast match finders | `include/wimlib/hc_matchfinder.h`, `bt_matchfinder.h`, `matchfinder_common.h` | Repeated input, maximum distances, deterministic valid candidate chains |
| Optimal parsing/suffix structures | `include/wimlib/lcpit_matchfinder.h`, `divsufsort.h`, `src/lcpit_matchfinder.c`, `divsufsort.c` | Empty/small blocks, suffix ties, path-cost overflow, compression level memory bounds |

Assign decompressor and compressor owners separately within each codec while keeping shared constants immutable. Optimal parsing changes compressed output, so validation checks decoded content and valid format, alongside measured ratio/memory, rather than asserting accidental byte identity.

### Writing and mutation

Build `WritePlan`: selected images, source blob ownership, deduplication, raw-copy eligibility, target compression, solid group packing, layout offsets, boot metadata, XML and integrity. Separate layout planning from serialized commit. Implement regular new-file write, descriptor write, overwrite/append/rebuild, solid write, pipable stream write, split, join, and reference/delta export in that order. Follow `write.c` for in-place overwrite versus rebuild semantics; interruption may leave recoverable data differently from an atomic rename. Test failure at each write/flush/seek stage and retain input sources.

Export and template/reference operations must retain backing-resource lifetimes across original-handle release where required. Split validates GUID/part-count/part-number relationships and reference resolution, including absent parts and repeated parts. Joining must not assume the caller orders parts. Streaming extraction must never fall back to seeking or buffering the whole archive to disguise a broken pipable implementation.

### Filesystem behavior

Capture and apply are phased: scan metadata and identities; acquire/hash payloads; create directories; materialize streams; create hardlinks/reparse objects; apply metadata/security; finalize directory timestamps. The upstream order and warnings govern flags where order is observable. Test file changes between scan and read and partial extraction failures. Paths must distinguish archive path semantics from host path semantics, capture exclusions, wildcard/path-list matching, case folding, and reparse-point fixups.

Unix backend preserves the original's supported symlinks, hardlinks, modes, ownership, times, UNIX data, and xattrs according to flags and filesystem capabilities. Windows backend uses native wide paths and OS APIs for security descriptors, named streams, sparse/compressed/encrypted files, junctions/symlinks, short names, case behavior, object IDs, and supported reparse tags. Include VSS snapshot capture and privilege behavior (`win32_vss.c`, `win32_capture.c`, `win32_apply.c`). Include WIMBoot/WOF and compact extraction behavior supported by the baseline; gate tests by actual OS support, recording unavailable capabilities rather than treating those tests as passed.

NTFS volume mode requires volume geometry, MFT/attribute/runlist parsing, stream/sparse handling, security descriptors, names/link relationships, and writes compatible with the filesystem. A native backend is a major independently staffed workstream with corruption/failure tests and disposable disk images. Reusing ordinary mounted-directory capture does not implement raw-volume mode.

FUSE mount needs lazy resource reads, inode identity, attribute views, read-only and read-write flags, overlay persistence, busy/unmount handling, commit/discard behavior, and interprocess communication from the original. Ensure writeback invokes the same writer/core contracts, not a second archive implementation.

## C ABI and deployment

Keep the original public header as the acceptance oracle. Use opaque heap-owned C handles and `#[repr(C)]` only on public records/unions; check all size/alignment/offsets with C probes on each target. Match platform `wimlib_tchar`, enum/flag widths, `size_t`, calling convention, nullable pointer contracts, callback return values, and borrowed/owned output buffers. An idiomatic Rust API is additional, not a substitute for ABI compatibility.

On GNU 32-bit x86 builds, including MinGW GCC with `__i386__`, upstream `WIMLIB_ALIGN_STACK` applies `force_align_arg_pointer` to realign the incoming stack. `extern "C"` alone does not establish this property. Provide an audited target-specific entry shim or verified compiler strategy and test calls from deliberately ABI-valid but SIMD-misaligned legacy callers; this must remain compatible without moving WIM algorithms into C.

Probe Windows 32-bit timestamp `tv_sec` width (32 bits), Windows 64-bit width (64 bits), high timestamp fields, and Unix native `timespec` layouts separately. C bitfields need byte-pattern probes, not just `offsetof`; trailing stream arrays need allocation/stride and callback-data access tests. Preserve exported spelling, including `wimlib_set_image_descripton`. Verify baseline Windows `libwim-15.dll` naming and both MSVC/MinGW import-library consumers.

Audit every allocation that crosses the boundary: global allocator replacement, ownership and deallocator requirements, transient callback arrays, returned XML/compression objects, and string lifetimes. Ordinary Rust-owned buffers cannot be returned where callers free with a configured C allocator. Keep custom-allocator objects behind an audited abstraction; test failure on each allocation ordinal and realloc behavior. Invalid non-null foreign pointers cannot be safely validated; document and match the C caller contract.

Catch unwinding at each exported function in unwind builds and translate unexpected internal failures without unwinding into C. Panic-abort builds do not become recoverable through `catch_unwind`; build policy must be explicit. Reject invalid arguments before dereferencing. Keep unsafe code narrowly at OS/ABI boundaries with safety comments, and deny `unsafe_op_in_unsafe_fn`.

Produce `libwim` shared/static artifacts, upstream-compatible package metadata/header installation, correct SONAME/DLL names and exports, and both compile-and-link and already-compiled consumer tests. Rebuild the original `programs/imagex.c` against the Rust library to exercise unchanged API clients. Run a prebuilt original CLI with the Rust library substituted through an isolated loader path. The final CLI gate also includes Rust CLI behavior if a replacement executable is shipped: command names/options, exit codes, stdin/stdout/diagnostics, scripts and aliases. Avoid global system-library replacement during testing.

## Parallel work packages and dependency DAG

Each package owns its paths, interface tests, source-derived fixtures, regression tests, and ledger entries. Each begins with a failing behavioral test. Dependencies below mean a merged contract/mock is sufficient to start; complete implementations are required to close the integration gate.

| ID | Package and owner boundary | Dependencies | First failing tests | Completion evidence |
| --- | --- | --- | --- | --- |
| P0 | Oracle/ABI/coverage harness | None | Alternate library cannot satisfy symbol/layout probes | Versioned baseline, fixture hashes, original suite runner, ledger |
| P1 | Types/errors/encoding/progress contract | P0 | Image/flags/string/callback mappings differ | C probes plus exact error/callback differential traces |
| P2 | Header/raw resources/blob lookup | P1 | Decode provided WIM fixtures and malformed ranges | Original fixtures and truncation/overflow corpus |
| P3 | XPRESS encode/decode | P1 | Original blocks fail Rust decode | Both encoder/decoder cross-directions and standalone C tests |
| P4 | LZX encode/decode | P1 | Window/block/transform vectors fail | All supported sizes/levels, malformed corpus, cross-directions |
| P5 | LZMS encode/decode/match finder | P1 | Solid/LZMS fixture cannot decode | Solid/non-solid cross-reader tests and API boundary probes |
| P6 | Metadata/security/tags/graph | P1, P2 | Canonical metadata fixture manifest differs | Hardlink/ADS/reparse/security/name preservation tests |
| P7 | XML/integrity/registry | P1, P2 | Property update/integrity corruption expectations fail | Exact property/error tests, original verify agrees |
| P8 | Resource streaming/solid/pipable/cache | P2, P3–P5 contracts | Random chunk/pipe reads mismatch | Seek-forbidden pipes, bounded memory, corruption tests |
| P9 | Writer/overwrite/split/join | P2, P6–P8 | Original reader rejects Rust uncompressed output | All codecs/layouts bidirectional write/apply and fault injection |
| P10 | Core update/export/reference/template | P6–P9 contracts | Original update/extract scenario differs | Original update suite, failure/state and lifetime tests |
| P11 | Unix capture/apply/path matching | P6, P8, P9 contracts | Original common tree scenario loses metadata | Unix original suites in all cross-directions |
| P12 | Windows capture/apply/VSS/WOF | P6, P8, P9 contracts | Native tree comparator reports lost ADS/security | Windows original suites plus VSS/WOF privilege matrix |
| P13 | NTFS volume backend | P6, P8, P9 contracts | Raw image scenario differs | Original NTFS suite, filesystem checks, disposable-image evidence |
| P14 | FUSE mount/writeback | P8–P11 contracts | Original mount test fails reads or commit | Mount suite, fault/busy/unmount tests |
| P15 | Complete C ABI and package | P1 onward incrementally | Original client fails each new symbol | Full symbols/layouts/allocator and prebuilt-consumer gates |
| P16 | CLI integration/performance/release | P9–P15 | Unchanged upstream command test fails | Full suite/platform ledger, deployment and benchmarks |

Start P0/P1 immediately; then run P2, P3, P4, P5, P6 contract work, and P7 in parallel. Backend teams start against mock image/resource interfaces while codec/writer work continues. P15 starts with initialization and standalone codec ABI rather than waiting until the end. P13 and P14 are long-lead packages and must be staffed early. Do not assign all teams edits to a central `lib.rs`; a designated integrator owns workspace manifests and facade exports.

At each integration point, require interface producer and consumer tests, then merge; avoid speculative cross-team type changes. A contract revision needs migration of all consumers in the same integration branch. Every work package names its outstanding optional-platform gates explicitly.

Work-package handoffs must contain concrete shared types and operations:

| Package | Exported handoff to consumers |
| --- | --- |
| P0 | `BaselineManifest`, `ScenarioId`, `OracleResult`, fixture loader and subprocess/loader-path runner; no runtime dependency from production |
| P1 | `ImageIndex`, `BlobHash`, `WimName`, `FileTime`, `WimError`, `ProgressEvent`, validated operation flag records, cancellation contract |
| P2 | `Header::parse/encode`, `ResourceDescriptor`, `BlobTable::parse/encode`, source-part identity and validation functions |
| P3/P4/P5 | Common compressor/decompressor factory, block codec traits, configuration validator, exact required-memory query; codec-specific implementations behind this interface |
| P6 | `Metadata::parse/encode`, `ImageGraph`, stable inode/dentry/stream/security IDs, traversal and manifest builder |
| P7 | `ImageProperties::get/set/remove`, XML parser/writer, integrity builder/verifier, registry key/value readers |
| P8 | `ResourceReader`, `ResolvedBlob`, `BlobStore`, bounded cache, sequential pipable reader state machine |
| P9 | `WritePlan::build`, `Writer::write/overwrite`, split-part planner and join validator; commit outcome explicitly represents handle invalidation after successful overwrite |
| P10 | `UpdateTransaction`, `export_image`, reference/template operations, image selector/iterator; source-resource retention uses explicit owned references |
| P11/P12 | `CaptureBackend`, `ApplyBackend`, capability records, host-path conversion, privilege/options validators; Windows-specific VSS and WOF providers |
| P13 | `NtfsVolume::open`, checked volume reader/writer, capture/apply adapter implementing the common backend traits; volume mutations restricted to disposable test targets until acceptance |
| P14 | `MountSession`, lazy file view, overlay object store, commit/discard request channel and unmount result |
| P15 | Exactly 72 C exports from the public inventory, C-visible structs/unions/callbacks, configured allocator abstraction, handle ownership facade |
| P16 | Original-compatible argument parser/commands, package layouts, artifact/install manifest and unified acceptance report |

Keep internal interfaces versioned in a design file and compile producer/consumer mocks in CI. The table does not authorize inventing new public C symbols: P15 exports remain the exact upstream contract.

## Original-test-driven workflow

1. Build the frozen C reference out of tree and establish actual `make check` results before interpreting failures. Inspect `Makefile.am`: default scripts are `tests/test-imagex`, `test-imagex-capture_and_apply`, and `test-imagex-update_and_extract`; mount and NTFS scripts are configuration-dependent. `tests/wlfuzz.c` is an extra program, not part of `make check`. Apply the test strategy's harness strengthening before accepting a baseline: `test-imagex-capture_and_apply` places its failure action under an `/usr/bin/tree` availability check, so comparator failure can be swallowed when that optional diagnostic tool is absent. Record an assertion-preserving adapter that always fails on comparator error and only conditions the diagnostic tree dump. Keep the original script/hash as provenance and report both original and strengthened runs.
2. Copy the original test tree into an isolated harness workspace. `test_utils.sh` resolves `../../wimlib-imagex`, and scripts `cd tests`; arrange that topology or change only the harness executable binding in a recorded adapter. Preserve test bodies and assertions. Give each run a disposable root. Never run cleanup scripts in source/media directories.
3. Compile original `tests/tree-cmp.c`, `win32-tree-cmp.c`, `set_reparse_point.c` and required test-support helpers for their intended targets. `src/test_support.c` enables random tree generation/comparison only with `--enable-test-support`; if its test-only symbols are used, either retain a separate C oracle helper or implement compatible test support explicitly.
4. For each upstream scenario, run C capture→C apply as environmental control, C capture→Rust apply, Rust capture→C apply, and Rust capture→Rust apply. Use original tree comparators. Also compare archive metadata with canonical manifests; filesystem limitations can conceal metadata loss.
5. Convert each discovered mismatch into a minimized regression before the fix. Retain input archive, expected C result/error, callback trace, platform/privileges, configuration, and fixture checksum. Use generated filenames/test IDs to make runs reproducible, including random-tree seeds.
6. Keep unchanged original suites as acceptance tests while adding Rust unit tests at parser/codec boundaries, property tests, C ABI probes, and focused differential cases for gaps. Do not rewrite original assertions to fit the new behavior.

Add missing coverage for LZMS/solid, standalone compressor/decompressor contexts, allocator customization/failure, all progress messages and cancellation sites, corrupted/truncated metadata/resource/XML/integrity, incremental update failure, imported/reference-resource lifetimes, unusual UTF-16 names, pipes with fragmented reads, >4 GiB offsets and sizes, and Windows VSS/WOF. The upstream `test-imagex` explicitly describes itself as sanity testing rather than comprehensive coverage.

Use `tests/wims` as original fixture inventory; classify every fixture by features and expected success/failure before adding it to a gate. Preserve security descriptor fixtures as raw bytes as well as interpreted semantics. External Microsoft readers and real Windows boot/install tests add interoperability evidence but cannot replace API tests.

Suggested harness commands are specifications to implement, not available commands today: `cargo test -p wim-format --locked`, `cargo test -p ms-compress --locked`, `cargo test -p wim-oracle --test abi --locked`, and `cargo run -p wim-oracle -- upstream --reference <C-build> --candidate <Rust-build> --suite <name>`. Harness output must include exact invocation, hashes, test IDs, skips with reasons, exit codes, duration, and artifact paths. Run rustfmt, all-target/all-feature Clippy with warnings denied, and meaningful tests on each merged package.

## Concurrency, resource bounds, and failures

Use a coordinator plus bounded chunk work queues. Workers own codec state and input/output buffers; sequence numbers restore on-disk ordering. Bound memory by queue slots × chunk capacity plus codec workspace, not input archive size. Solid grouping may require spooling; expose its limit and test oversized blobs. Hashing, compression, and raw copying share cancellation tokens, and cancellation drains/joins workers before releasing handles. Never invoke foreign callbacks while holding internal locks.

Preserve observable progress phases and counters under parallel execution. Differential tests compare required event order and values, while allowing timing/coalescing variation only where the public contract permits it. Test callback abort during scan/read/compression/write/extraction/verification and join, callback context lifetime, zero/automatic thread counts, thread-creation failure, and competing independent handles. Add model tests for shared cache/reference state and deadlock stress tests; do not mark raw pointers `Send` to make compilation succeed.

Fault-inject allocations, short reads/writes, EINTR-equivalent interruptions, no space, permission failures, source changes, missing references, corrupt compressed data, rename failures, and worker failures. Tests assert upstream-consistent errors and permitted resulting state, not blanket atomicity. Match upstream memory-reporting APIs with checked formulas for supported codec contexts.

## Milestones and release gates

| Milestone | Deliverable | Gate |
| --- | --- | --- |
| M0: baseline/contracts | Fixture inventory, source/version manifest, public ledger, ABI probes, interfaces | Original suites run with configuration/skips documented; no feature omitted from ledger |
| M1: read/verify | All ordinary/solid/pipable readers and all decoders; metadata/XML/integrity | Original corpus, malformed corpus, streaming and cross-decoder tests pass |
| M2: native write/core | All encoders, new/overwrite/descriptor writers, image mutations/reference/export/split/join | Bidirectional original readers/writers; fault and lifetime tests pass |
| M3: Unix complete | Capture/apply/extract/update plus mount and configured NTFS support | All applicable original Unix suites pass, additional gap tests pass |
| M4: Windows complete | Native metadata, VSS, WIMBoot/WOF, privileges, supported extraction modes | Original Windows comparators/suites and targeted optional-feature gates pass on disposable VMs |
| M5: ABI/deployment | Full `libwim` exports/layouts, allocators, CLI/package artifacts | Unchanged rebuilt clients and prebuilt clients load and behave correctly on each target |
| M6: replacement accepted | Full ledger, robustness/performance evidence, licenses, support matrix | No required stubs/skips; all platform/configuration gates pass; documented deviations resolved |

These milestones are readiness gates, not duration estimates. Estimate effort only after baseline inventory and prototype codec/NTFS/Windows results; codec optimization and raw NTFS can dominate the schedule. A Linux-only milestone is useful progress and cannot be advertised as the completed replacement.

## Current integration contracts for parallel implementation

The partial C implementation uses one opaque `WimHandle`. Source images retain
their original one-based metadata identity through `HandleImage::Source`; newly
added empty images own their actual metadata bytes through `HandleImage::Empty`.
Exported images use `HandleImage::Owned`, with stable origin identities to
reject duplicate re-export and independently retained backing data. Payload
decoding remains lazy: exporting a corrupted payload must not introduce an
earlier checksum failure than upstream. Destination writes and verification
must remain usable after source release. GIFT transfers only resources absent
from the destination; original source-side transfers can survive a later XML
failure even though destination image state rolls back. Preserve this observed
failure side effect rather than assuming the entire operation is transactional.
Deletion changes the current image vector and XML numbering together. It must
not renumber original metadata identities. Empty images begin with a null root;
writing materializes a directory root and updates their pending digest. Lookup
callbacks report the original synthetic descriptor state rather than confusing
serialized metadata length with an in-memory blob's reported length.
Pending images retain shared metadata through an `Arc<Mutex<PendingMetadata>>`;
export and re-export clone this ownership. Root materialization, timestamps and
the digest become visible to every live owner. Snapshot bytes while holding the
lock briefly; release it before callbacks or I/O. Future update operations must
enforce the original shared-metadata mutation restriction, error 86, and then
test ownership transitions after each handle is freed.

File-backed blobs remain in the table after their reference count reaches zero,
matching upstream's treatment of persisted counts as untrusted. Future capture
and reference implementations must distinguish file-backed, newly captured,
external and unhashed descriptors before extending deletion or lookup behavior.
Verification checks all retained data blobs, including unreferenced file-backed
ones; it checks metadata hashes with metadata error semantics and does not
substitute integrity-table checking for stream verification.

XML element names, attributes and text now own platform bytes. Linux setters
and getters preserve arbitrary nonzero bytes. Safe string accessors validate
UTF-8. Original platform conversion also accepts WTF-8 representations of
unpaired UTF-16 surrogates; UTF-16 serialization must preserve these units while
rejecting other malformed byte sequences with error 31. Writer and mutation work must use this tree directly rather
than introducing a getter-only cache of changes. Windows lossless wide strings
and locale-dependent conversion remain open requirements.

The C writers consume separate ordinary and solid codec/chunk settings.
Current serializers buffer staging and output; this is a prototype boundary,
not the memory contract for release. Streaming, callbacks, failure timing,
allocation hooks and overwrite/reference semantics must be integrated without
weakening the milestone gates above. Consult the public API ledger and current
evidence before claiming that symbol presence establishes API parity.

Directory callbacks borrow a complete public-layout record and its string,
security and stream storage only for the duration of the call. Traversal must
resolve canonical inode metadata and report missing resources according to the
selected flags before exposing stream descriptors. Raw XML APIs return the
original resource rather than reserializing the mutable property tree. Their
allocation and host C stdio ownership must eventually use the registered
allocator and platform contracts. Split/join adapters consume current image
selection, XML and resource counts, preserve source media, and distinguish
pending images from unchanged stored metadata. Progress-bearing variants must
implement callback lifetime and cancellation before being exported.

Each handle retains a copied callback/context registration in a `Cell`. The
caller owns callback code and context storage. Image-verification events read
the registration again at each begin/end; stream verification retains a phase
snapshot, so unregistration does not invalidate the in-flight context. Integrity
opening uses its supplied registration and installs it on the handle only after
success. Status 1 maps to cancellation error 76; any other nonzero status maps
to 77. Release shared metadata locks before dispatch. Never retain an exclusive
borrow of the entire handle across callbacks that can replace its registration;
stage through shared borrows and take fresh exclusive borrows only for commits.
Writer, split and join events must surround real work; a sequence emitted after
serialization does not establish progress, cancellation or failure parity.

The writer callback oracle now supplies concrete integration gates. Ordinary
writing emits initial stream progress before reading file data, then metadata
begin/end events with NULL information pointers and actual integrity calculation
events. Abort leaves a partially written target at the phase's actual byte extent;
the retained red probes distinguish header-only, completed-data, completed-metadata
and XML-written states. A new writer resource plan must keep metadata selection
separate from lazy payload consumption, reuse encoded stored resources for
unchanged/default policies, and recompress when flags or codec/chunk changes
require it. Progress compressed-byte counts must describe real encoded output.
Join progress counts distinct data source parts, rather than the output header's
single part. Preserve the no-progress safe writer interfaces as wrappers while
connecting the same serialization work to real incremental sink/callback hooks.

## Performance, provenance, and remaining decisions

Benchmark original and Rust release builds on the same CPU/storage: codec throughput/ratio/peak workspace by level, capture/apply throughput, tiny-file metadata overhead, dedup-heavy exports, solid random extraction, pipe peak memory, and thread scaling. Select numeric acceptance thresholds after obtaining baseline distributions; do not invent parity numbers before measurements. Correctness gates precede tuning. Record CPU-feature disabled runs inspired by `tests/test-imagex` SHA-1 verification coverage.

Determine target architectures and upstream-supported feature configurations from the baseline and configuration scripts; CI includes native Linux and Windows rather than assuming cross-compilation proves behavior. Pin dependencies and audit for native WIM/codec delegation. Review source-license obligations per file (`COPYING`, `COPYING.LGPL`, `COPYING.GPLv3`), test fixtures, and any direct algorithm translation before distributing. Native Rust does not by itself change copyright obligations.

Resolve before M0 closes: permitted OS/FUSE adapter dependencies; exact supported OS/architecture matrix; baseline artifact naming; whether compatible test-only symbols are distributed; and whether shipped scope includes the executable and helper scripts as well as the library. Native NTFS remains required by default. Pending these decisions, the plan preserves the broadest upstream library scope, including optional backends, rather than treating ambiguity as permission to drop features.
