# Native opaque WIM handle C APIs

`wimlib_create_new_wim`, `wimlib_free`, and `wimlib_open_wim` now use native
Rust-owned state. Handles own the selected fixed header, mutable XML, original
file bytes when present, an already parsed lookup table, a canonical filename,
stable platform-string storage,
and separate ordinary/solid output compression settings. Re-parsing the borrowed
archive from owned bytes avoids self-referential Rust structures. The owned
lookup descriptors permit C callback iteration without reparsing or heap
allocation; newly created empty handles have no table. New handles
have the original input compression/chunk defaults (none/zero), while requested
compression affects output settings only. Original `create_new_wim` leaves the
GUID zero; GUID generation occurs at write time (`src/write.c:2785`), so the
unchanged-header probe checks that all four newly created handles retain zero
GUIDs rather than incorrectly generating them early.

The unchanged `/tmp/wimlib/include/wimlib.h` compiles
`scripts/wimlib/probe-handles-api.c` against both libraries.
`python3 scripts/wimlib/check-handles-api.py` captures four original compressed
fixtures, constructs explicit invalid-header/XML/lookup/incomplete/split cases,
and includes all eight original `tests/wims/*.wim` regression files. The current
[differential result](differential.json) matches all 288 observations across 26
input paths. Every failed create/open checks that the output pointer stays
untouched. The synthetic corpus preserves source bytes in its private temporary
directory; no production media is modified. Initial differential evidence is
preserved as an observation baseline, not claimed as a failing TDD run.

Three Rust contract tests cover output-pointer failure behavior and precedence,
null free, empty XML ownership, and separate input/output defaults. Targeted
strict Clippy passed. Root workspace checks provide combined integration evidence.

Open validates public flag bits, resolves a real filename, normalizes invalid
boot indices, validates compression/chunks, rejects requested writable access for
split/header-readonly/filesystem-readonly inputs, optionally checks integrity,
and checks XML image count and lookup records. Metadata and filesystem contents
remain lazy, as in the original API. Unix writeability uses `access(W_OK)` like
upstream; the Windows path uses native wide paths but remains unvalidated.

These exports are partial, not complete drop-in claims. Whole input files are
buffered, so memory consumption is not original bounded file-backed behavior.
Read buffers use fallible reservation and handles use checked allocation, but
XML parsing/string/path/cache allocations do not yet obey original custom
allocator callbacks. Global-init options, error diagnostics and errno fidelity,
progress-aware opening, adversarial allocation failures, concurrent input-file
changes, lazy file-backed handles, Windows behavior and complete lifecycle with
capture/write/overwrite/export references remain outstanding. Handle layout is
private; C callers must only use opaque pointers allocated by this candidate.
