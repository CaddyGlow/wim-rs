# Native Rust wimlib replacement: documentation set

This set specifies a full native Rust replacement for the library in
`/tmp/wimlib`, including its public C interface. The initial native workspace
and evidence are tracked in [implementation-status.md](implementation-status.md).
The replacement is not yet complete.

The reference checkout identifies itself as **wimlib 1.14.5**, commit
`cd5e231c348c255ae5088873b5a66ee0eb96fa07`. The public header checksum and
individual function declarations are recorded in
[public-api-inventory.json](public-api-inventory.json). Source-relative paths in
these documents refer to this reference checkout, not the windows-uup source
tree. Preserve that checkout and its fixtures as the differential oracle.
The [upstream test inventory](upstream-test-inventory.json) records SHA-256
checksums for the shipped suites, fixtures, fuzz targets/corpora, example-test
tools and Automake test configuration before building the oracle.

Read these documents in order:

1. [API reference](../wimlib-api-reference.md): public operations, flags,
   callbacks, ownership, error behavior, and compatibility obligations.
2. [Test strategy](../wimlib-test-strategy.md): upstream suite inventory,
   execution prerequisites, missing coverage, and differential release gates.
3. [Implementation plan](../wimlib-rust-implementation-plan.md): native modules,
   shared interfaces, dependency ordering, parallel work packages, and TDD.

## Meaning of drop-in replacement

The target is the supported behavior of the pinned library, including ordinary,
split, solid, and pipable WIMs; all compression formats; editing and resource
references; integrity and verification; capture and apply; platform metadata;
mounting; callbacks; and low-level compression APIs. A successful subset is a
development milestone, never the final compatibility claim.

An existing C application must be able to compile against the original header
and link against the replacement. An existing supported binary must load the
replacement with the expected library naming, exported symbols, calling
conventions, and structure layouts. ABI checks must cover actual target
architectures. CLI behavior is additionally needed to run the original imagex
tests; CLI tests alone do not establish the library ABI.

Native Rust means the WIM model, parser, writer, codecs, and operations execute
in Rust. Calling the original library from production code does not meet this
target. OS interfaces necessarily remain foreign interfaces. Any proposed
external filesystem dependency must be explicitly assessed against the native
scope; optional features cannot silently disappear to avoid that assessment.

Compatibility is observable semantics, not universal byte-identical output.
Compression heuristics and layout can vary while preserving valid interoperable
files. Conversely, valid WIM output alone is insufficient: error values,
callback cancellation, ownership, resource sharing, and platform restoration
also matter. Compare byte layout wherever the format or API promises it, and
compare decoded content and metadata elsewhere.

## Documentation verification

From the windows-uup repository root:

```sh
python3 scripts/wimlib/audit-api.py --source /tmp/wimlib --check docs/wimlib-api-reference.md
python3 scripts/wimlib/audit-api.py --source /tmp/wimlib
python3 scripts/wimlib/inventory-tests.py --source /tmp/wimlib
```

The first command checks that every public function declaration and every
`WIMLIB_*` identifier extracted from the header appears in the reference.
This is a coverage aid, not proof of
semantic correctness. The second prints declarations and the header SHA-256;
compare it to the checked-in inventory before using a different checkout.

## Evidence required before the implementation can be called complete

Maintain a ledger with one row per public function, flag family, progress
message, public type, codec, WIM variant, and platform feature. Each row must
name its implementation owner, upstream evidence, regression tests, differential
results, and remaining limitations. Mark unsupported or unexecuted gates as
incomplete, even when the host suite passes.

Release requires all applicable upstream tests, additional direct C API and ABI
tests, cross-reader/cross-writer tests, malformed-input and fuzz regression
tests, allocation-failure and cancellation tests, and real platform gates.
Validate Windows behavior in disposable guests and mount/NTFS behavior in
isolated environments. Preserve input media and fixtures. A successful archive
write does not prove a Windows installation matches its intended target.

The source's `COPYING` offers GPLv3+ for the project and LGPLv2.1+ for the library
when third-party licensing permits; linking libntfs-3g affects that choice.
Track the provenance and license of ported code and test fixtures as part of
each work package. This set does not assign a new license to upstream material.
