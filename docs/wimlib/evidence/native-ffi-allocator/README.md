# Registered C allocation dispatch

> Current behavior (2026-10-05): `ms-compress` now uses Rust's global allocator
> directly for codec workspace vectors. C callbacks still control outer codec
> handles and WIM-owned allocator-aware storage. The codec workspace hook traces
> and parity claims below are historical evidence, not current guarantees.
> Current factory tests inject failures in both allocation families and verify
> NOMEM, untouched output pointers, cleanup, and allocation-free codec reuse.


Source baseline: unchanged wimlib 1.14.5, commit `cd5e231c348c255ae5088873b5a66ee0eb96fa07`, `/tmp/wimlib/src/util.c` and `textfile.c`, with the public declaration from `/tmp/wimlib/include/wimlib.h`. C probes link either the independently built original or the native Rust cdylib. No original library is linked into production Rust code.

`allocation.rs` implements `wimlib_set_memory_allocator` and actual malloc/free/realloc dispatch. Each NULL callback independently restores the corresponding host C default. A failed zero-size malloc retries with size one; realloc normalizes zero size to one. Callbacks run outside the registry lock and can reenter the setter. Existing allocations must remain compatible with whichever free/realloc callback is subsequently installed, as in upstream.

Text-file raw disk buffers, stdin buffers and their growing reallocations, returned text buffers, returned XML buffers, and the XML extraction buffer free use this dispatch. Successful returned buffers belong to the caller, which must release them with the configured compatible free callback; host C free remains correct under the defaults. Failure preserves output pointers/lengths according to the observed API contract.

`differential-first.json` records **1,921 matching C observations**: ten payloads, disk/stdin, all eight independent combinations of custom/default hooks, failures at allocation events 1–5, zero-size malloc failure, and a reentrant setter callback. The comparison includes allocation sizes/order, free/null-free events, pointer ownership identities, status, errno, output publication, content, and termination. UTF-16 malformed input and unpaired surrogate input are included. Initial and grown stdin failure returns 24 and frees the current buffer; disk/output allocation failure returns 39.

`xml-differential.json` records **24 matching C observations** for raw XML get and extraction, loaded original/native-written WIMs, empty WIMs, allocation failure, advertised size, untouched output pointers, and configured callback ownership. `xml-empty-errno-red.json` preserves the mismatch found before restoring upstream's EBADF side effect for the absent input descriptor on an empty WIM.

`text-default-regression.json` records the existing **2,440 matching observations** with default allocators. `tests.log` and `clippy.log` record passing FFI tests and strict Clippy.

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim
python3 scripts/wimlib/check-allocator-abi.py --native target/debug
python3 scripts/wimlib/check-xml-allocator-abi.py --native target/debug
```

This is **partial allocator compatibility**, with actual caller coverage. Internal Rust Vec/String allocations in parsing, conversion, XML decoding, metadata, codecs, writer/extraction work, and other APIs are not globally routed through this C dispatcher. The traces establish the listed API boundaries, not all allocations made by the library. Platform evidence is Linux x64 with host libc; Windows allocator/CRT ownership and other Unix behavior remain gates. The tests do not prove total Rust allocation failure recovery or a custom allocator's compatibility with arbitrary Rust-owned state. Additional facade/codec/handle migration must be validated separately.

## Opaque handle and codec-context storage

Handle creation/opening and compressor/decompressor context storage now use the registered malloc/free callbacks. Real uninitialized handle storage is allocated before file opening and parsing and held by RAII; successful construction publishes that same allocation. Every error or cancellation releases the storage without exposing an incomplete handle. Codec factories initialize global runtime before parameter validation, allocate their outer storage before codec-specific maximum-size validation, and release it when codec construction fails. Destruction runs native Rust destructors and then invokes the currently registered free callback, including after callback-family replacement.

`context-differential-first.json` records **840 matching outer ownership contract observations** across valid/invalid codecs, maximum sizes, NULL output, first-allocation failure, callback replacement, reentrant setter calls, runtime initialization, successful first-allocation publication, and outer free counts/family. Both original and native raw hook traces are retained. **These are not exact allocation-size or full inner-trace matches:** native opaque objects have actual Rust layouts, and upstream codec/XML workspace allocations have not all migrated. No unused allocations or padding were added to imitate upstream traces.

`context-inner-oom-red.json` deliberately exercises the next allocation failure. It records **1,260 observations with 63 remaining contract mismatches**, establishing a concrete unresolved gate rather than claiming blanket allocator compatibility. Native internal Rust-owned allocations bypass these callbacks, so some second-allocation failures observed by upstream are not yet seen by native APIs. These red results remain expected until substantive inner allocation migration is complete.

`open-progress-regression.json` records **336 matching open integrity/progress/cancellation observations**, including filename lifetimes and untouched output pointers. `context-tests.log` and `context-clippy.log` record passing FFI tests and strict Clippy after the outer storage changes.

At the outer-storage milestone, the next required paths included handle backing/metadata collections, format XML trees, LZMS decoder workspaces, and encoder vectors. The decoder migration below resolves the direct FFI decoder workspace subset; handle/XML and encoder paths remain. Per-call decode/encode allocations and retained codec workspace must use a real owned allocator-aware representation with meaningful OOM behavior; a Rust process-wide GlobalAlloc replacement is not used here.

```sh
python3 scripts/wimlib/check-context-allocator-abi.py --native target/debug
# Expected to fail while inner allocations remain unmigrated:
python3 scripts/wimlib/check-context-allocator-abi.py --native target/debug --include-inner-failure-gates
```

## Decoder workspace migration

The lower `wim-memory` crate provides a safe initialized typed-buffer API with fallible allocation, capacity-bounded resizing, and owned callback release. Its small unsafe implementation validates pointer alignment and keeps pointer construction/destruction outside the codec crate. `wim-codec` retains `forbid(unsafe_code)`. Safe Rust callers retain the default Rust allocation strategy; FFI context factories inject wrappers that dispatch dynamically through the registered malloc/free callbacks. No process-wide Rust allocator replacement is used in production.

XPRESS contexts now own and use their Huffman table; LZX contexts own and use their code-length/history tables and canonical codes. LZMS contexts own their frequencies, lookup tables, parent/sort/length workspaces, bounded binary heap, decision probabilities, and x86 target history through this strategy. The bounded heap replaces BinaryHeap without allocating during rebuild. None of the registered allocations are empty tokens: the decoder accesses the allocated data during decoding. Every workspace construction failure releases preceding buffers and the outer object without publishing a handle.

`decoder-workspace-differential.json` records **540 matching decoder ownership/OOM contract observations**, including the formerly red second-allocation cases. The overall second-allocation matrix improved from **63 to 39 mismatches**; `context-inner-oom-after-lzms.json` and `context-inner-oom-after-decoders.json` retain the intermediate and current gates. Original/native allocation sizes and full allocation graphs remain different and are explicitly retained in the observations. Upstream often allocates one packed private context; native uses real typed buffers, so higher allocation-event failure indices are not claimed to match upstream's graph.

`decoder-reuse-differential.json` records **80 matching repeated C decode calls**, successful output hashes and malformed-input statuses, zero additional registered allocation/free events during reuse, and balanced ownership after freeing all contexts. LZMS cases use output sizes 32 and 1024. `decoder-small-output-upstream-crash.json` separately preserves the original library's SIGBUS on the zero-alphabet/small-output LZMS domain while native returns safely, even with upstream SSE4.2 disabled. This reproduces the already documented tiny-output decoder gate and is not counted as a matching observation.

The existing FFI factory OOM regression now injects each **actual registered outer/workspace allocation** for all three codecs and verifies NOMEM, untouched output, and zero live hooked buffers after failure. It also retains a Rust allocation counter/failure loop for any remaining Rust allocations reached by factory construction. Safe default codec tests continue to inject and recover from every Rust workspace allocation, and reused LZMS decoding remains allocation-free. `decoder-workspace-tests.log`, `decoder-factory-oom-tests.log`, and `decoder-workspace-clippy.log` record passing tests and strict Clippy.

Typed buffers preserve `Send` for Send elements and `Sync` for Sync elements. The unsafe callback constructor requires compatible callbacks that remain callable and support cross-thread ownership. Tests prove both codec context types retain Send+Sync, move default contexts between threads, and move a callback-owned buffer between threads before freeing it.

Remaining mandatory paths are **compressor retained/per-call workspace**, handle backing/metadata ownership, XML trees and decoding, and archive reader/writer/extraction code that still constructs codecs with their default Rust strategy. Default Rust-owned allocation is deliberately preserved for safe APIs; it does not imply registered C hook coverage when such APIs are reached internally from FFI. Compressor second-allocation cases and handle/XML second-allocation cases remain red. Decoder size/graph equivalence and higher failure-event-index comparisons also remain partial.

```sh
python3 scripts/wimlib/check-context-allocator-abi.py --native target/debug --include-inner-failure-gates --decoder-only
python3 scripts/wimlib/check-decoder-workspace-abi.py --native target/debug
```

## Compressor workspace migration

XPRESS now retains hash heads, previous links, token storage and bitstream output. LZX retains E8 preprocessing bytes, hash links, tokens and output. LZMS retains preprocessing/history, match links, adaptive Huffman code/table/heap state, decisions/probabilities and both coding streams plus assembled output. These are actual workspaces used during compression and reset on every block, including after capacity failures. Their fixed capacities follow the configured maximum; every allocation is fallible and construction unwinds all previously allocated buffers on failure. Caller capacity does not trigger unbounded output allocation.

The FFI compressor uses `compress_borrowed` to copy retained encoded bytes directly to the caller's buffer. Safe Rust APIs keep their owned Vec result through a separately fallible output copy, and safe callers can explicitly use the borrowed retained-output method. Codec unsafe-forbid and cross-thread ownership remain intact. The ownership enum stays inline in the single fallible context allocation; its documented large-enum lint allowance avoids introducing an unregistered Box allocation and an extra OOM boundary.

`codec-workspace-differential.json` records **1,080 matching compressor/decompressor C ownership/OOM contract observations**, including first- and second-allocation failure, errno, untouched outputs, replacement/current callback free, and reentrancy. `context-inner-oom-after-compressors.json` records the full **1,260-case** matrix with **15 remaining mismatches**, all handle/XML construction paths. The previous 39 mismatches are retained in `context-inner-oom-after-decoders.json`, showing the concrete compressor subset progressing from red to green.

`compressor-reuse-interop.json` records **90 native encoded blocks read by the unchanged original C decoder**, across all three codecs, two input patterns and five level/destructive configurations. The C client also compares 120 factory configurations with original and verifies **1,278 registered allocations and 1,278 frees**, with **zero new allocations during reused compression**. Level tuning remains fixed strategy; this verifies valid output and ownership rather than identical compressed bytes or upstream parse decisions.

`compressor-factory-oom-tests.log` injects every actual registered workspace/outer allocation for all three encoders, verifies NOMEM and untouched pointers, and checks zero live hooked buffers after each failure. It retains failure injection for any Rust-owned allocation reached by the facade. Reused compression also tracks Rust allocation events and requires zero. `compressor-workspace-tests.log` records passing memory/codec/FFI tests; `compressor-workspace-clippy.log` records passing corresponding strict checks.

Remaining mandatory allocator paths are handle backing/metadata ownership, XML trees/decoding, and reader/writer/extraction paths that construct safe codecs with their default Rust strategy rather than explicitly injecting the registered C strategy. Higher failure-event-index and exact allocation-size/graph parity remain partial: upstream's packed codec-private workspace differs from native typed owned buffers. Safe owned-output APIs deliberately copy into Rust Vec storage. Original compression-level policy, eager workspace sizing/query parity, and platform gates remain independently partial.

```sh
python3 scripts/wimlib/check-context-allocator-abi.py --native target/debug --include-inner-failure-gates --codec-contexts-only
python3 scripts/wimlib/check-compressor-workspace-abi.py --native target/debug
```

### XML document ownership milestone

The real non-Copy XML document root now resides in `wim-memory::Owned<Element>`.
Safe format APIs use default Rust storage; facade create/open inject the current
registered callback strategy through new fallible parse entry points. Callback
failure returns NOMEM and leaves output unchanged. Successful foreign ownership
runs the complete element destructor before freeing its allocation through the
current registered family, without holding the registration mutex. This is actual
AST storage, not an allocation token. Nested children and their text are destroyed
as part of that owned root. The generic boundary preserves Send/Sync with the
value's corresponding bounds and the allocator's documented thread contract.
Tests cover non-Copy destruction on success and allocation failure, and transfer
of owned foreign values between threads.

`context-inner-oom-after-xml-root.json` records **1,260 matching C contracts**:
the previous 15 fail-at-2 differences are now green, including create and open,
replacement callback families, reentrant callbacks, errno, output invariants,
and outer ownership cleanup. The previous red matrices remain evidence of the
migration. Allocation byte sizes and complete inner traces still differ from
upstream. `xml-root-tests.log` records passing memory/format/facade tests and
`xml-root-clippy.log` records the strict corresponding Clippy check.

**Remaining mandatory allocator paths:** XML element names, attribute vectors,
child vectors, text byte strings, image-index vectors, parser UTF-16/WTF-8
conversion scratch, serialized XML output and staging clones; handle backing
bytes, lookup records, image vectors, Arc ownership state, property-string cache,
sets/maps and mutation-owned resources. `Owned::clone` deliberately creates
independent default Rust storage, so facade clone/staging callers still require
an explicit fallible allocator-aware cloning migration. The intended next graph
step is a lower-crate generic growable non-Copy collection with initialized-length
tracking, allocate/move/free growth, element destruction on rollback, and fallible
nested clone methods. No completion claim follows from the now-green second-
allocation matrix.

### Nested XML collection milestone

`wim-memory::Collection<T>` now owns non-Copy initialized values in registered
storage. Growth allocates real replacement storage, moves initialized values,
and releases the previous allocation. Failure preserves existing ownership;
remove/retain/clear/drop destroy each retained value exactly once. Lower-boundary
Send/Sync follow the element's bounds and callback thread contract. Tests exercise
foreign growth, failure preserving content, non-Copy removal/destruction, partial
fallible-clone rollback, and cross-thread transfer.

XML retained names, text, attribute names/values, child and attribute collections,
image indices, and the parser's temporary index reconciliation collection now use
that storage. `XmlInfo::try_clone` preserves the injected strategy and is used by
image selection and append, including writer staging through image selection.
Default safe `Clone` still creates default Rust storage. The new
`xml_allocation` regression faults every real parse and clone callback allocation,
checks rollback balance, and verifies the original document remains byte-identical
on clone failure (`xml-allocation-rollback.log`). Full memory/format/facade tests
and strict Clippy pass (`xml-collection-tests.log`, `xml-collection-clippy.log`).

The previous **1,260 second-allocation contracts remain green** in
`context-inner-oom-after-xml-collections.json`. The expanded handle-only matrix
faults indices zero through 20 and records **1,224/1,260 matching contracts** in
`context-inner-oom-through-20.json`. Its 36 differences are exclusively create
failures at indices 4, 5, and 6; all native successful callback allocations balance
with nonnull frees, including every error exit. The real remaining create graph
contains separately owned XmlInfo storage and blob-table control/buckets. This
expanded red gate supersedes any impression that the second-allocation matrix
established complete allocator parity. Parser UTF-16/WTF-8 conversion scratch,
serialization output, default Clone callers, handle backing/lookup/maps/sets/Arc,
and broader operation-specific allocation ordering remain mandatory migrations.

### Actual XmlInfo and blob-table ownership milestone

Facade handles now own separately allocated `Owned<XmlInfo>` storage. A lower
`Reservation<T>` reserves the actual object before constructing its graph and
releases uninitialized storage on every failed construction path; initialization
transfers exactly one destructor to the completed owner. The real owned blob
map uses a separately allocated control object and initialized open-addressed
buckets, initially 64 slots. Existing resource consumers read this map directly;
export/reference insertion uses fallible reserve/insert, and removal, inspection,
and overwrite clear operate on its actual retained entries. Growth moves real
owned key/value entries into callback-backed replacement buckets. No unused
allocation placeholders are involved. Tests cover collision chains, tombstones,
growth failure preserving entries, owned-value destruction, initialized and
uninitialized reservations, and thread transfer.

Both matrices now pass: **1,260/1,260 previous cases**
(`context-inner-oom-after-handle-graph.json`) and **1,260/1,260 handle cases through
failure index 20** (`context-inner-oom-through-20-after-handle-graph.json`). The
previous 36 red create cases are green; every successful native malloc trace has
a corresponding nonnull free in both matrices. Raw trace sizes and full ordering
still differ, including upstream attempting blob-table construction after failed
XML construction. Full memory/format/facade tests and strict Clippy pass
(`handle-graph-tests.log`, `handle-graph-clippy.log`). Independent open-progress
and property matrices also pass (`open-progress-after-handle-graph.json`,
`properties-after-handle-graph.json`). These are host Linux contracts; Windows
platform allocation/text behavior remains separately gated.

Remaining migrations include retained archive backing and parsed lookup vectors,
handle image vectors and Arc state, property-pointer cache, removed/dirty sets,
operation preparation maps/vectors, XML conversion scratch and serialization
output, default clone callers, and codec per-call helper/default-strategy callers.
Later failure indices and operation-specific allocation order must extend the
matrix. Matching indices through 20 establishes this milestone, not complete
registered-allocator coverage.

### Retained backing and lookup milestone

Actual retained archive bytes and exported/reference source snapshots now use
callback-backed byte collections. The original read buffer remains fallible Rust
scratch. Conversion occurs only after the existing header/flag/compression
prechecks and integrity callback dispatch, preserving proven failure/cancellation
precedence; scratch is dropped after conversion. This transient double-buffer
peak and the future streaming/preflight reader remain explicit gates.

Retained lookup resource/blob/metadata vectors, lookup hash index, input-record
reconciliation, and overlap-validation scratch now use injected Collection/Map
storage through `Archive::open_with_allocator`; default `Archive::open` keeps a
default Rust strategy. The parser fault regression exercises every actual hook
allocation, verifies retained lookup resolution, and checks every rollback leaves
zero live callback allocations. Complete memory/format/facade tests and strict
Clippy pass (`backing-lookup-tests.log`, `backing-lookup-clippy.log`), and open
progress/cancellation contracts remain green (`open-progress-after-backing-lookup.json`).

The expanded matrix now faults zero through 100: **6,012/6,060 contracts match**,
with all native successful mallocs balanced by nonnull frees. The previous
6,060-case baseline had 72 differences; this real migration turns 24 cases green
(open failure indices 55–62). The remaining 48 differences occur only at open
indices 63–78: native currently makes 62 registered allocations in that fixture,
upstream makes 78, so later fail indices cannot trigger a native allocation.
Both raw matrices are retained (`context-inner-oom-through-100-before-backing.json`,
`context-inner-oom-through-100-after-backing-lookup.json`). The checker now asserts
native callback ownership balance in addition to comparing C status/errno/output
contracts. Remaining Rust allocations and legitimate packed-collection graph
count differences are identified separately; no allocations are added solely to
match upstream event counts.

Image/owner vectors, pending shared state, filename/platform buffers, sets,
property-pointer cache, initial file-read scratch, serialization/conversion
scratch and operation-specific preparation still need routing. The milestone is
host Linux evidence; Windows behavior and complete allocator coverage remain gated.

### Shared handle state and retained filename milestone

Actual image/owner collections, reference-counted owner/pending/backing controls,
lazy dirty/removed sets, fallible set checkpoints, and retained platform filename
bytes now use the injected allocator. Shared cloning changes a reference count
without issuing allocations; final release dispatches through the current free
callback. Filename cloning is explicitly fallible. Allocation preparation precedes
image/reference-count commits, preserving rollback on hook failure.

`context-inner-oom-through-100-after-shared-handle.json` records **6,024/6,060**
matching contracts, with every native successful malloc balanced by a nonnull
free. This turns 12 further cases green. The remaining 36 differences occur only
at open failure indices 67–78: this native retained graph makes 66 registered
allocations and the original makes 78. Both earlier matrices remain as evidence;
allocation events are never added solely to equalize graph counts.
`shared-handle-tests.log` and `shared-handle-clippy.log` pass, and
`open-progress-after-shared-handle.json` preserves cancellation/progress contracts.

Pending metadata byte vectors, captured trees/resources, property-pointer caches,
initial input scratch and its double-buffer peak, temporary path/progress text,
XML conversion/serialization scratch, default clone callers, and operation
preparation still need migration. Registered allocator coverage and Windows
behavior remain partial.

### Live pipe factory ordering milestone

The unchanged-header `probe-pipe-allocator.c` injects actual malloc/realloc
failures, tracks live callback allocations, observes caller descriptor ownership,
and drains a duplicate descriptor only after the API returns to measure bytes
consumed. The checker freezes both libraries and records partial output trees.
Before the fix, 17 of 186 focused cases differed: allocation 2 failed before
native accepted the input, while upstream had read its 208-byte header and
closed the descriptor.

The pipe facade now allocates actual outer handle storage before taking input
ownership, validates the header, prepares actual retained image/table storage,
and directly retains the parsed XML. It no longer constructs an unrelated empty
XML graph. All **186/186** focused contracts match
(`pipe-oom-before-factory.json`, `pipe-oom-after-factory.json`). Independent
progress/recovery/IO comparison remains **2,268/2,268** exact, including stderr
(`pipe-extract-after-factory.json`). Facade tests and strict Clippy pass.

The deliberately broader failure-through-100 matrix records **572/606** matches
(`pipe-oom-through-100-after-factory.json`). Its 34 remaining differences are
valid-input indices 53–86: four consume input at different allocation phases;
later failures do not fire after the native registered graph ends at 56 events,
while upstream continues through 86. Every observed native failure and cleanup
leaves zero tracked callback allocations. Metadata parsing, extraction preparation
and filesystem/path temporaries still use unregistered Rust allocation paths;
these need real migration rather than count-matching hook calls. Allocator
coverage remains partial.

### Retained metadata and extraction preparation milestone

`Metadata::parse_with_allocator` now owns actual security descriptor references,
node/stream/child collections and DFS/cycle/name/hard-link reconciliation scratch
through the injected allocator. Names borrow unchanged resource bytes rather than
allocating duplicate byte keys. Safe default parsing retains the default Rust
strategy. Explicit destructor-lifetime adapters release borrowed graphs before
backing bytes move or handles mutate; no infallible clone was added.

The pipe facade uses `with_image`: selected metadata bytes remain locally owned,
one injected graph is validated before later image metadata is skipped, and a
callback consumes that graph while its bytes remain live. This eliminates the
previous default-allocator validation graph and the second parsing pass without
self-referential storage. Whole-image stream counting reads canonical inode IDs
and no longer allocates an unused full layout.

Extraction owns callback-backed selected nodes, terminated leaf names, directory
indices/descriptors, parent/hash indexes, stream consumer groups, progress text,
open stream file collections and reparse buffers. Fallible insertion releases
owned descriptors/values on failure. Transfers use allocation-free reversal/pop
to preserve traversal order without quadratic shifting. Temporary name/UTF-16
conversion, platform filename conversion, fallback concatenation, canonical path
resolution and other helper vectors remain explicit scratch allocation gates.

`pipe-oom-after-preparation.json` improves the broad matrix from 572 to
**583/606** matches (11 further cases green); the checker independently asserts
zero live native allocations after every cleanup. Native now owns 85 actual
registered events in this fixture, compared with 86 upstream. Remaining 23 cases
are phase/read-volume differences at 53–71 and 84–85, upstream status49 versus
native39 at index81, and original failure versus native success at86. Packed
collection graph counts and real unregistered scratch paths remain separate
gates; no allocation is issued merely to match a trace.

The every-index metadata fault regression validates unchanged parent/inode/name/
child semantics and zero live allocations after every actual failure.
`pipe-preparation-tests.log` passes memory/format/facade tests;
`pipe-preparation-clippy.log` passes strict full-workspace Clippy. Independent
Linux pipe recovery/progress/IO/stderr comparison remains **2,268/2,268** exact
(`pipe-extract-after-preparation.json`). Windows and complete allocator coverage
remain partial.

### Root-path lookup and conversion scratch milestone

Upstream failure81 is **PATH_DOES_NOT_EXIST (49)**. Its unchanged-library
backtrace resolves `wimlib_malloc(size4)` → `convert_string`
(`encoding.c:241`) → `get_dentry` (`dentry.c:818`) →
`do_wimlib_extract_paths` (`extract.c:1841`). It is the actual UTF-16
conversion of the WIM root path, rather than target permissions. The converter
returns NOMEM; `get_dentry` returns NULL and its caller maps that failure to49.
`pipe-original-fail81-trace.log` and `pipe-original-fail81-source.log` retain
this proof.

Whole-image preparation now uses the shared literal selector and an actual
allocator-backed terminated UTF-16 path, consumed by component/name lookup.
Conversion/lookup failure maps to49; independent preparation allocation failure
continues to map to39. Callback errno is preserved on conversion OOM. The
corresponding native root-conversion failure currently occurs at67 and returns
49 with errno123 and zero live allocations; its frozen-library trace is retained
in `pipe-native-root-lookup-failure-trace.log`. Different packed ownership graphs
mean corresponding operations are not necessarily the same ordinal event.

UTF-16LE XML converts directly into callback-backed WTF-8 scratch, eliminating
the intermediate unit vector. Retained leaf names convert directly into their
actual terminated callback-backed buffers. Both converters preserve unpaired
surrogates, validate before allocation, and have real hook-failure/rollback
regressions. Pipe targets borrow valid caller path bytes, eliminating an
intermediate PathBuf. Progress platform text is constructed directly in its
actual retained collection; anchored root open reuses that terminated target
buffer. Root directory creation uses libc mkdir with mode0777 and the same
EEXIST/errno handling, eliminating standard-library pathname conversion scratch.
Fallback concatenation and retained canonical root paths also use real owned
callback-backed buffers; canonicalization system-library scratch remains a gate.

`pipe-oom-after-conversion.json` records **585/606** equal contracts (two more
green than the preceding583 milestone), **zero native ownership failures**, and
86 actual successful registered allocation events in both libraries. The21
remaining differences are valid-input indices54–71,81,85–86. Equality of event
counts does not establish equality of ownership phases: read-volume and
operation/error mapping differences remain explicit, including original81 versus
native67 for root conversion. No padding hook calls are issued.

With the final frozen artifact97d15b71…, both independent matrices remain exact:
**4,440/4,440** flag/target policy cases and **2,268/2,268** lifecycle/recovery/IO
cases, including stderr (`pipe-policy-after-conversion.json`,
`pipe-extract-after-conversion.json`). Strict full-workspace Clippy and
memory/format/facade tests pass (`pipe-conversion-clippy.log`,
`pipe-conversion-tests.log`). Other conversion/reparse/serialization helpers,
filesystem/canonicalization scratch and other FFI operation graphs still require
migration. Complete allocator and Windows behavior remain partial.
