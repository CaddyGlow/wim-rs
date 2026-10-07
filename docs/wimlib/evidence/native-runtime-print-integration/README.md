# Runtime and printing integration snapshot

`workspace-tests.log` records 279 passing all-feature Rust tests, zero failures
and zero ignored tests. Formatting, strict all-target/all-feature Clippy and
`git diff --check` also pass. Commands use the native workspace manifest, locked
dependencies and `target` explicitly.

The API ledger has four host-verified and 50 partial functions. Runtime and
diagnostics add five partial entries with 130 original/native lifecycle cases;
printing adds two with 137 exact stdout cases. See their dedicated evidence
directories for reproducible C comparisons and remaining gates.

The currently built Linux library contains 56 required public symbols. Two
additional exports, `wimlib_extract_image` and `wimlib_join_with_progress`, are
under active differential validation and deliberately have no implementation
claim yet. Consequently the strict export ledger audit currently reports those
two discrepancies. This snapshot is not a completed API milestone or evidence
of full drop-in compatibility.
