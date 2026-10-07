# Resource references, text loading and progress ABI milestone

The candidate exports 47 of the 72 required functions: four host-verified
information functions and 43 partial implementations. Twenty-five functions
remain unimplemented. `export-audit.json` verifies the ELF symbols against the
ledger; `api-audit.json` verifies the full documentation inventory of 72 functions
and 271 constants. Symbol presence does not establish full compatibility.

`workspace-tests.log` records 274 passing all-feature Rust tests. Strict
all-target/all-feature Clippy, formatting and tracked whitespace checks pass.
This milestone adds five partial exports and connects verification to actual
callbacks:

- [Registration/verification](../native-ffi-progress/README.md): 45 exact ordered
  C callback comparisons, including cancellation, replacement, unregistration,
  phase snapshots and corrupt-resource error timing.
- [Opening with progress](../native-ffi-open-progress/README.md): 336 exact C
  cases for integrity checking, completed chunks, malformed inputs, cancellation,
  invalid statuses and untouched outputs on failure.
- [Resource references](../native-ffi-references/README.md): two APIs, 62 cases
  and 823 observations; 29 source-free native outputs independently verified
  and applied by original wimlib. Covers missing split parts, compressed/solid
  resources, duplicate handling, rollback, globs and inherited integrity callbacks.
- [Text loading](../native-ffi-text-file/README.md): 2,440 C comparisons for
  original encoding detection, buffered stdin, exact bytes/count/terminator,
  C allocation ownership, surrogate conversion and Linux error/errno behavior.

The integration refreshes the 17-case verification matrix and the 336-case
opening matrix; records are in this directory. Other individual evidence links
preserve their unchanged-header clients, original observations and corrected
red failures. Resource payload/order normalization is restricted to upstream's
unspecified content enumeration; ordered progress messages remain ordered.

Release gates still include allocator hooks/OOM, bounded file-backed memory,
compression tuning, complete callback/cancellation behavior, source resource
reuse, write failure timing, overwrite, filesystem capture/apply/mounting,
Windows/other platform ABI/runtime behavior and packaging. Writer callback
red probes now prove missing lifecycle events and partial-target extents; those
gates remain open until callbacks surround real lazy read/encoding/output work.
The full implementation plan retains every feature and platform requirement.
