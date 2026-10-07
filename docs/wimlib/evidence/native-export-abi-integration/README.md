# Export, traversal and split/join ABI integration milestone

The candidate exports 42 of the required 72 symbols: four host-verified
information functions and 38 partial implementations. Thirty functions remain
unimplemented. `export-audit.json` checks actual ELF symbols against the complete
ledger; `api-audit.json` independently checks the documented 72 declarations
and 271 constant identifiers against the unchanged original header.
`workspace-tests.log` records 261 passing tests with all features enabled;
`clippy.log` records strict all-target/all-feature Clippy. Formatting and
tracked diff whitespace checks also pass.

The integration adds six partial exports:

- [Export](../native-ffi-export/README.md): 386 cases and 12,676 original
  observations; 271 destination archives independently verified and applied
  after source release. Lazy payload ownership preserves checksum failure
  timing; shared pending metadata preserves root materialization across owners.
- [Directory traversal](../native-ffi-dir-tree/README.md): 2,168 original/native
  cases and 32 C layout observations, including streams, security, hardlinks,
  reparse data, missing resources, callback stopping and WTF-8 names.
- [Split/join](../native-ffi-split-join/README.md): 96 split comparisons and
  360 cross-library joins independently verified and applied.
- [Original XML access](../native-ffi-xml-data/README.md): exact raw resource and
  caller-owned C stdio comparisons across 11 codec/layout combinations.

The owned XML tree now preserves WTF-8 surrogate representations when reading
and writing UTF-16 resources. Safe Rust string conversion remains fallible.
[Property evidence](../native-ffi-properties/README.md) records 1,498 existing
observations and 55 raw-byte/WTF-8 write/reopen observations. Conversion tests
also round-trip every individual UTF-16 code unit.

Integration refreshes compare 240 writer cases, 17 verification cases, 13
mutation cases and 216 information-client observations. The corresponding JSON
records are in this directory. Original writer verification/apply uses upstream's
supported `WIMLIB_DISABLE_CPU_FEATURES=sse4.2` setting. The default-reader crash
named in `write-differential.json` is preserved in the
[original writer evidence](../native-ffi-write/README.md); a configured pass does
not establish default optimized-reader parity.

Full drop-in parity remains incomplete. Allocator hooks, failure injection,
streaming memory bounds, compression tuning, callbacks/cancellation, overwrite,
external references, complete flags, filesystem capture/apply/mounting, Windows
text/ABI/backends and packaging remain release gates. Shared pending ownership
does not establish future shared mutable filesystem-update behavior.
