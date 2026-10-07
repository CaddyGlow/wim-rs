# Native implementation status

The native engine is [crates/wim](../../crates/wim/README.md), a root workspace member.
The full target remains the [implementation plan](../wimlib-rust-implementation-plan.md).

The workspace contains three WIM packages:
`crates/wim`, `crates/wim-format`, and `crates/wim-types`,
under the root Cargo workspace. Their licenses are preserved explicitly.
The public Rust API returns an owned `Info` with typed compression; raw C entry
points and layouts live under `wim::ffi`. Low-level implementation and its
validation hooks live under the documented-hidden `wim::engine` namespace.
Existing Rust callers of raw root-level symbols must use `wim::ffi`; exported
The shared-library artifact name is retained; custom allocator registration
(`wimlib_set_memory_allocator`) is deliberately removed. Rust containers use
the global allocator, while C-owned output buffers use the fixed host C runtime.
The format core supports `no_std + alloc`. Historical allocator-hook and symbol
counts below describe the artifacts recorded at that time. Active scripts and
commands use root `target/`; recorded logs and JSON artifact manifests are kept.

Native Rust API separation (2026-10-05): the safe `Wim` API owns a boxed engine
handle and calls typed Rust operations directly, with `forbid(unsafe_code)`
enforced in `src/api.rs`. C entry points adapt raw pointers to the same shared
engine operations. Native cancellation uses a scoped Rust function rather than
an extern-C trampoline and restores previous registrations on error or unwind.
C write, overwrite, capture, extraction, and reference adapters avoid exclusive
handle borrows across callbacks that may replace progress registration.
The shared update engine now uses Rust deletion and renaming helpers, rather
than calling back through C exports.

Validation after this separation: 651 WIM/format/types/windows-uup tests pass
with 12 ignored; strict WIM/format/types/windows-uup Clippy, formatting, diff
checks, and MSVC all-target/all-feature compilation pass. An earlier strict
workspace Clippy run passed; the final rerun encountered concurrent unrelated
`archive-core/src/sevenz_backend.rs:396` work with a `CreateOptions` test
initializer missing `zip_encryption`. Tests cover native capture/add rollback
and retry, operation-panic cancellation cleanup, existing C callback replacement,
and both Rust and C handle ownership. Linux exports remain 71 ledger symbols
plus two optional test helpers. The original-decoder probe still passes 90
roundtrips and 120 factory comparisons. These checks do not renew Windows
runtime, installation, or servicing evidence.

Allocator-removal validation (2026-10-05):
`cargo test -p wim -p wim-format -p wim-types -p windows-uup --all-features --locked`
passes 648 tests with 12 ignored. The portable configuration passes 127 tests
with no ignored tests. Bare-metal compilation for `thumbv7em-none-eabi`, WIM
MSVC all-target/all-feature compilation, full-workspace strict Clippy, portable
strict Clippy, formatting, and diff checks pass. The rebuilt Linux test-support
library exposes 71 ledger symbols plus two optional test helpers, with no missing
claimed or unexpected exports; the removed allocator setter is absent.
The independent compressor probe passes 90 original-decoder roundtrips and 120
factory comparisons. Allocator-hook differential drivers are explicitly retired;
the original C probes and frozen evidence remain for upstream analysis.
These are host tests and compile checks, not renewed Windows runtime or
installation/servicing evidence. Box/Arc allocation does not promise recoverable
OOM; checked container growth still reports allocation errors where supported.

Historical consolidation validation (before allocator removal): 458 tests pass (3 ignored) for `wim`, `wim-format`,
`wim-types`, `wim-memory`, and `windows-uup`, including doc tests, in an isolated
`target/wim-refactor` directory with `--all-features -- --test-threads=1`.
Formatting passes for these packages, strict all-target/all-feature Clippy
passes for the four WIM crates, and their MSVC Windows cross-target compile
check passes. The separate fuzz workspace's tests pass. The Linux library retains all 74 baseline `wimlib_*` symbols (including
optional test helpers) and `libwim.so.15`; a C create/free smoke test passes.
The earlier complete workspace Clippy and MSVC compile checks passed, but
later complete-workspace validation was affected by concurrent unrelated work:
three `cpcopy` live-progress CLI failures saw a binary without that feature,
and a later CLI Clippy attempt encountered unresolved `windows-delta` ARM64
module references. These files are outside this refactor. The previously
observed `capture_write` errno assertion fails intermittently with parallel
host tests and passes with the serialized run; no errno policy was changed.
Windows runtime and installation gates remain unchanged.

Before consolidation, the 2026-10-04 Rust API refactor renamed the engine package and directory from
`wim-ffi` to `wim`. `windows-uup` now uses owned `Wim` archives, validated
`ImageIndex` values and typed errors; its WIM adapter forbids unsafe code.
Committing changes consumes the archive. Compatible C exports and library
artifact names are retained. Active commands and source paths use the new name;
historical logs retain their original names.
Validation: root host tests pass (610 passed, 7 ignored), the WIM workspace's
all-feature host suite passes (276 passed, 1 ignored, including Rust API and
doc tests), formatting and strict all-target/all-feature Clippy pass in both
workspaces, and the CLI passes an MSVC Windows cross-target `cargo check`.
The first WIM suite attempt failed an existing `capture_write` errno assertion
(`EAGAIN` instead of `EINVAL`); the focused rerun and complete suite rerun passed.
These checks do not add Windows runtime, installation or frozen ABI evidence.

Four information functions are verified against an unchanged-header Linux C
client. Sixty-eight additional exports are partially implemented with differential
evidence; their remaining compatibility gates are documented below. The current
packages provide archive readers and ordinary, solid and pipable serializers.
All three original portable CLI scripts and the comparator-fatal variant now
pass with the unchanged original CLI linked to a frozen native library; see
[the full upstream evidence](evidence/native-full-upstream/README.md).
[Local Linux packaging](evidence/native-linux-packaging/README.md) supplies the
actual `libwim.so.15` SONAME, header and audited artifact manifest.
Capture, complete filesystem apply and write/overwrite policies, and drop-in API
compatibility remain unfinished.
The [controlled performance optimization follow-up](evidence/performance/optimization.md)
measures optimized original and Rust libraries on identical 18 MiB mixed-file
input. All 30 final measured samples pass independent original verification and
content comparison. Relative to the retained baseline, Rust XPRESS capture/write
is 2.65 times faster, ordinary LZMS capture/write 2.12 times faster, and ordinary
LZMS application 3.08 times faster. Current ordinary LZMS writing takes 1.13 times
original time; LZX writing takes 0.57 times with 3.01% larger output. Other modes
and memory use remain behind original. The final artifact passes 365 workspace
tests, strict Clippy, all three strengthened portable suites and a 60-second
unchanged randomized run (703 native iterations, independent original baseline
2,460). These warm-cache, single-thread synthetic measurements do not establish
performance on Windows installation media.
The [public API status ledger](public-api-status.json) lists all 72 required
exports, with four host-verified, 68 partial and 0 unimplemented for Linux.
[Windows compilation and DLL linkage](evidence/native-windows-cross-build/README.md)
pass for MSVC and GNU. The earlier GNU artifact has 70 exports, with the two
pipe entry points missing. Both pipe entry points now compile and link in a
new frozen GNU DLL; actual descriptor and read-volume comparisons are pending.
Earlier 65-export artifacts retain their frozen evidence. [Selected Windows guest comparisons](evidence/native-windows-abi/README.md)
match 278 behavior/layout rows and 65 exports using the same MinGW caller.
The expanded runtime probe matches all 44 measured Rust/C layout values; the
earlier 38-value records are preserved. Both native builds write WIMs
that the original reader verifies and applies. Complete Windows filesystem, NTFS,
ABI and distributable-package gates remain open.

The following table records early workstream milestones. Later integration
evidence below and the public API ledger take precedence for current coverage.

| Workstream | Milestone evidence | Remaining work at that milestone |
| --- | --- | --- |
| P0: oracle | Original library built in `/tmp/wimlib-native-oracle`; portable suites pass 3/3, no skips, with comparator assertion strengthening | Full ABI offsets/bitfield probes, complete case ledger, platform oracles, failure injection |
| P1: types | All 81 numeric errors, compression values/chunk validation, lossless UTF-16 names; 7 Rust contracts pass | Flag families, timestamps, events, allocator and handle contracts |
| P2: headers/lookups/resources | Seekable pipable header selection; ordinary and solid lookup resolution; ordinary, seekable pipable and solid resource reading; 24 header and 22 lookup C comparisons | Partial ranges: 176 C comparisons; file-backed ranges: 8 contracts, 176 C comparisons and sparse 1 TiB read-volume gates; whole archive index, pipe streaming and recovery pending |
| P3: XPRESS codec | 20 edge contracts and 4,150 original-C differential cases pass (1,105 successful outputs, 3,045 rejections) | Native encoder added (632 original-reader outputs including 67 expanded blocks); level/context/FFI APIs, robustness, performance and full compressor parity pending |
| P4: LZX codec | 32 contracts and 3,355 C differential cases (637 successful outputs, 2,718 rejections) | Native encoder added (64 original-reader cases); level/context/FFI, robustness and performance gates pending |
| P5: LZMS codec | 8 internal tests, 4 API tests and 1,938 C differential cases (721 successful outputs, 1,217 rejections); recoverable allocation failures | Native encoder added: 6 contracts and 119 original-reader cases; levels, custom allocator/FFI, robustness and performance gates pending |
| P6: metadata/XML/integrity | Metadata parsing and inode alias normalization with 40 C comparisons; XML parse/edit/serialization with 39 upstream comparisons; integrity SHA-1 calculation/verification with upstream fixtures | Owned metadata serialization added: 10 contracts and two C verify/apply cases; complete image operations, bounded I/O and ABI pending |
| P8: resource/archive writing | Resource serializer: 9 contracts, 18 original-C reads; ordinary archive serializer: 88 C comparisons; solid: 66; pipable: 88 including fragmented stdin; new-image builder: 24 verify cases | Streaming archive I/O, overwrite, write flags, callbacks and cancellation |
| P9–P10: image/split operations | Image select/reorder/delete/export: 8 C verify cases and 14 applies; split/join: 8 bidirectional cases and 24 original invalid-set observations | External reference contexts, complete flag policies, callbacks, transactions and ABI |
| P15: codec contexts/ABI | Safe contexts: 92 C factory + 18 boundary cases; four verified information exports; partial codec ABI: 36 decoder blocks, 120 compressor factory cases, 30 encoder cross-reads; memory query: 29,646 comparisons; handles/properties/info/lookup/traversal/export/raw XML/split/join/references/text/progress now exposed | Level tuning, allocation hooks, actual native memory budgets, lifecycle, platform ABI and remaining 25 exports |
| P11–P14/P16 | Architecture and source/test mapping documented | Operations, filesystem backends, mounting, ABI and deployment |

The native-reading workspace evidence records 102 passing Rust tests,
including corpus tests replaying 9,443 C decoder observations across all three codecs.
Successful differential records compare exact output bytes; rejection records
compare success/failure rather than internal Rust error variants. Strict
all-target/all-feature Clippy and formatting checks also passed.
At that earlier reading milestone, no Windows, FUSE or direct-NTFS gate had run.
The selected Windows guest evidence linked above is newer; real FUSE and
direct-NTFS implementation gates remain open.
The [native-image-reading evidence](evidence/native-image-reading/README.md)
records 128 passing workspace tests and 11 original-writer/native-reader
comparisons across all codecs and seekable layouts. Archive composition verifies
blob SHA-1, reads image metadata, checks XML image counts, and exposes integrity
checking. It buffers whole resources, including solid resources; it is not yet
a streaming or filesystem extraction implementation.

[Native writing evidence](evidence/native-writing/README.md) records ordinary
WIM output using all four native storage modes. The internal archive APIs
remain separate from the four verified C information exports.
The latest [resource-range evidence](evidence/resource-ranges/README.md) records
189 passing workspace tests at that milestone. The latest
[XML writer integration](evidence/xml-writing/README.md) records 210 passing
workspace tests and refreshed XML byte statistics in all writer layouts.
Solid/pipable writers and new-image construction
are documented in their respective evidence directories. See
[the ABI evidence](evidence/native-ffi-info/README.md).

Codec ABI evidence is tracked separately for
[decompression](evidence/native-ffi-decompress/README.md),
[compression](evidence/native-ffi-compress/README.md), and
[memory queries](evidence/native-compression-memory/README.md).
The memory query reproduces the original x86-64 allocation formula; it does not
yet bound allocations by the native encoders. The eight partial exports are
not counted as host-verified API parity. The internal image selector now has
[26 public-API comparisons](evidence/native-image-resolution/README.md).

The [codec ABI integration milestone](evidence/native-codec-abi-integration/README.md)
records 222 passing workspace tests, strict Clippy, and the 12-symbol ledger
audit. Reusable LZMS decoding now preallocates scratch at creation; repeated
decodes allocate nothing, and factory allocation-failure tests verify NOMEM
without changing caller output storage.

The [archive ABI integration milestone](evidence/native-archive-abi-integration/README.md)
records 234 passing tests and 31 actual exports. Its 19 new partial exports
cover handle creation/open/free, nine image-property operations, six header
information/output-setting operations, and resource lookup callbacks. Open
retains parsed lookups for allocation-free traversal. Full-file buffering,
Windows platform text, complete in-memory capture ownership, allocation
callbacks, lifecycle and platform gates remain incomplete.

The latest [mutable ABI integration milestone](evidence/native-mutable-abi-integration/README.md)
records 244 passing tests and 36 actual exports. It adds real empty-image
ownership/deletion, C path/descriptor writers and verification. Linux property
text now uses an owned byte XML tree: 1,498 property observations and 15 write
outcomes close the arbitrary-byte gap. Writer evidence covers 240 cases using
the original generic reader configuration; preserved default SSE4.2 LZMS
reader crashes remain explicit counterevidence. Verification matches 17
valid/corrupted cases, including retained resources after image deletion.

The latest [export ABI integration milestone](evidence/native-export-abi-integration/README.md)
records 261 passing tests and 42 actual exports. Six new partial exports cover
image export, directory traversal, original XML access and split/join. Export
matches 12,676 observations across 386 cases and retains lazy data after source
release; 271 outputs pass independent original verification/apply. Shared pending
metadata now materializes one root and digest across exported owners. Directory
traversal matches 2,168 cases and 32 public C layout observations. Split/join
matches 96 split cases with 360 cross-library verify/apply joins. Original XML
access matches 11 codec/layout cases. WTF-8 conversion preserves unpaired
surrogates in metadata paths and UTF-16 XML resources, with 55 raw-byte property
write/reopen observations. The previous mutable milestone's counts remain its
historical evidence, rather than a claim about the current complete workspace.

The latest [reference/progress integration milestone](evidence/native-reference-progress-abi-integration/README.md)
records 274 passing tests and 47 actual exports. It adds real progress
registration, opening with integrity callbacks, two resource-reference APIs and
the text loader. Verification now matches 45 ordered original callback cases;
opening matches 336 cases. Text loading matches 2,440 original outcomes including
stdin buffering, lossless surrogates, output ownership and Linux errno behavior.
Resource references preserve lazy data after source release and complete missing
split-part resources without importing images. Writer/split/join progress and
source-resource reuse remain required work rather than implied by registration.

Subsequent [printing comparisons](evidence/native-ffi-print/README.md) establish
137 byte-for-byte C stdout cases for header and image-information output.
[Diagnostics and runtime comparisons](evidence/native-ffi-diagnostics/README.md)
establish 130 fresh-process lifecycle cases and rerun 2,440 text-loader cases.
These seven additional API ledger entries remain partial. The 274-test milestone
above is a historical snapshot; it does not count ongoing writer and extraction
changes, whose integration checks are still in progress.

[Allocator dispatch evidence](evidence/native-ffi-allocator/README.md) adds one
partial API entry with 1,921 text-buffer hook observations, 24 XML-buffer hook
observations and 2,440 default-allocator regressions. The hooks now govern actual
returned buffers and their frees; internal Rust allocations and remaining API
contexts still require migration and failure injection.

[Path mutation evidence](evidence/native-ffi-path-mutation/README.md) records
5,121 matching C delete/rename cases across layouts, codecs and Unicode case
policies. Written archives pass 1,752 original verifications and 876 paired
original-reader applications with matching content and metadata snapshots.
Dirty-image statistics refresh fixes a preserved write mismatch. Both APIs
remain partial for allocation failure, Windows and broader metadata gates.

[Selected-path extraction](evidence/native-ffi-extract-paths/README.md) adds two
partial exports with 445 cases and 3,177 matching observations, filesystem
snapshots, stdout, path-list parsing and cancellation evidence.
[Update transactions](evidence/native-ffi-update/README.md) adds DELETE/RENAME
batch execution with 5,400 exact comparisons, including callback inspection and
rollback. ADD remains pending native capture graph integration.
[Overwrite](evidence/native-ffi-overwrite/README.md) adds actual append,
replacement and compaction policies: 1,653 of 1,680 cases match exactly, and 463
healthy native outputs pass original verification. The 27 solid metadata codec
differences remain explicit compatibility gates. The
[capture implementation](evidence/native-ffi-capture/README.md),
[pipe extraction](evidence/native-ffi-pipe-extract/README.md),
[template checksum reuse](evidence/native-ffi-template/README.md) and
[disabled-FUSE capability](evidence/native-ffi-mount-disabled/README.md) bring
the ledger to four host-verified and 68 partial functions. Every required symbol
is present in the Linux library. The three mount symbols only match disabled-FUSE build capability;
real mounting remains unimplemented. Capture evidence includes 182 scan cases,
24 graph inspection cases and 36 verification cases. Complete allocator,
streaming, platform and whole-library behavior remains unfinished.

## Oracle evidence

[Evidence directory](evidence/native-foundation/) contains the three upstream
suite logs, the Automake summary, original/patched script hashes, host ABI
sizes, comparator regression result, and fixed-header differential results.
The offline codec oracle and its generation/provenance records are in
[the codec fixtures](../../crates/ms-compress/tests/fixtures/README.md).
The oracle was configured with `--without-fuse --without-ntfs-3g
--enable-test-support`. These portable results establish the reference
environment; they are not passes for the Rust candidate.

The comparator regression deliberately supplies a failing comparator and forces
the optional diagnostic branch unavailable. Original wrapper returns 0;
strengthened wrapper returns 42. The copied script makes the error unconditional
and retains optional tree dumps. The preserved `/tmp/wimlib` source is unchanged.

Reproduce on a host with the stated build prerequisites:

```sh
bash scripts/wimlib/build-oracle.sh /tmp/wimlib /tmp/wimlib-new-oracle
python3 scripts/wimlib/strengthen-oracle.py /tmp/wimlib-new-oracle
python3 scripts/wimlib/check-oracle-assertions.py --oracle /tmp/wimlib-new-oracle
make -C /tmp/wimlib-new-oracle check TESTS='tests/test-imagex tests/test-imagex-capture_and_apply tests/test-imagex-update_and_extract'
cc -I /tmp/wimlib/include scripts/wimlib/probe-abi.c -o /tmp/wimlib-abi-probe
/tmp/wimlib-abi-probe
cargo build --manifest-path Cargo.toml --target-dir target --example header_status --locked
```

The original `probe-abi.c` measures host sizes and enum widths only. Later
Windows probes separately measure selected offsets and bitfield bytes against
the unchanged C header, with 44 actual Rust/C layout values matching in the
expanded runtime probe. Neither
probe establishes the complete platform ABI; keep each scope attached to its results.

## Next integration gates

Expand malformed-metadata and image-operation coverage, integrate bounded
file-backed resource access into archive handles, and implement compression
level tuning and streaming writers. Preserve
the four cross-reader/writer directions as writer work continues. Introduce C ABI
exports incrementally only with real implementations and allocator/lifetime
tests; do not add success-returning stubs to inflate symbol coverage.

[Windows C-runtime printing](evidence/native-windows-print/README.md)
matches both original text-mode and binary-mode raw output using the same
MinGW/MSVCRT caller. This selected result does not establish all locale and CRT
variants.

[Selected Windows capture](evidence/native-windows-capture/README.md) matches
21 original scan/configuration/cancellation cases, including default/strict ACLs.
Native NO_ACLS and default-ACL WIMs preserve DOS names and an empty image-root
name, verify/apply with independent original readers, and reproduce the measured
Windows payload, hardlink, attribute and owner/group/DACL results. Ten selected
privilege lifecycle cases and five multi-command ACL/DOS-name write cases also
match, including sticky cleanup state and first-inode security policy within a
capture session. Restricted tokens and broader NTFS metadata remain separately
tracked gates.

[Retained blob traversal order](evidence/native-ffi-lookup/order-plan.md) records
the source table capacity, bucket chains, insertion/growth and hash readiness
rules needed to close the remaining ordered writer-state observations. Existing
hash/size/reference/verification/retry comparisons do not erase ordering reds.
The native handle now retains allocator-backed bucket chains and insertion
history, including transactional update rollback. Broader ownership transitions
and the writer's mixed-image ordering cases remain under differential test.

[Original random-image generation](evidence/native-full-upstream/fuzz/README.md)
now has 288 exact C observations for 32 seeds across ordinary, pipable and solid
write/continue/cancellation variants. All 96 successful native outputs verify
and compare equal through the original library. The staged graph/payload oracle
also covers 128 seeds. These helpers are optional test support, not additional
public exports. The unchanged randomized upstream executable advances into
overwrite and split. Selected NO_SOLID_SORT and solid ordering controls now
match; subsequent genuine failures exposed captured reparse stream serialization
and pipable split policies. A subsequent 60-second run exposed an append-overwrite
header-version defect: retained solid resources were written under an ordinary
WIM header. The source-version preservation fix has a failing-before/passing-after
Rust regression and independent original-reader verification. The unchanged
randomized runner now completes the documented 60-second Linux gate with status
0 against both libraries: 200 native iterations and 6,212 original iterations,
with all ten Linux operation types selected. See the
[randomized evidence](evidence/native-full-upstream/fuzz/README.md).
This time-bounded pass does not establish exhaustive randomized, Windows, FUSE,
direct-NTFS or complete API compatibility. The seeds differ between libraries.

[Windows extraction](evidence/native-windows-extract/README.md) has a preserved
48-case original baseline, the pre-backend Unsupported red, and a first native
backend result with 38 exact cases and ten actionable differences. The backend
uses real handle-relative NTFS creation, hardlinks, resource writes, security
descriptors and FILETIMEs. The corrected backend matches all 48 cases, including cancellation output,
NO_ATTRIBUTES, ACLs, FILETIMEs and actual hardlink identity. Five additional
509-entry DOS-name and 500-entry cancellation controls match the original.
Twelve target-path controls and three extended long-path controls also match.
Real anonymous-pipe behavior remains under test; no complete Windows extraction
or pipe gate is claimed here.


Windows selected-path and stream extraction now share the path-selection layer
with Unix, preserving ancestors or flattening selected roots and limiting
hardlinks and resources to selected inodes. The backend restores named data
streams, empty streams, directory/root streams, reparse data and absolute-link
fixups, compressed data and sparse holes. Empty unknown placeholder records do
not block extraction. Windows capture and extraction now preserve EFS raw
ciphertext (including EFS-managed named streams), object IDs and binary NTFS EAs.
EFS raw streams use delete-on-close disk spools with bounded transfer memory.
Object-ID collisions follow upstream: extraction succeeds but cannot assign
the duplicate ID on the same NTFS volume. Other metadata failures remain errors.
Nonempty unknown streams and deprecated Linux xattr records remain explicit
unsupported cases. UNIX metadata remains outside the Windows fidelity profile. Ordinary named-stream capture outside EFS
is still unsupported.

[NTFS metadata evidence](evidence/windows-ntfs-metadata/README.md) records
bidirectional interoperability with original Windows wimlib, raw EFS and plaintext
hashes, exact binary EA bytes, object IDs, timestamps, attributes and DACLs, plus
a regression covering direct capture extraction before WIM serialization.

[Windows stream extraction evidence](evidence/windows-extract-streams/README.md)
records real Windows regression tests and an original-DLL differential against
a native-written WIM. This is a focused extraction gate, not full reconstruction,
servicing, installation correctness or comprehensive NTFS compatibility. Ordinary
Linux extraction ignores ADS and cannot preserve Windows security descriptors;
direct archive export/repack retains these records and blobs. The original's
Linux direct-NTFS apply backend has no Rust equivalent here.

[Real UUP pipeline evidence](evidence/uup-pipeline-20261004-final/README.md)
records Windows base reconstruction from the complete local payload fixture,
corrected boot preparation, independent original-wimlib verification and one
audited SSU installation persisting as Installed after commit and remount.
The run exposed and fixed empty-directory child-list serialization: WIMMount
classified zero-offset empty directories as files, preventing CBS from creating
its session store. A failing-then-passing DISM regression covers that behavior.
The [bounded solid-chunk cache](solid-resource-cache.md) and retained writer
lookups address repeated source decoding and lookup parsing. This scope does not
establish complete update closure or an installed requested Windows target.
