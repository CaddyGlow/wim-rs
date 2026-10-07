# Windows compilation and linkage

The native workspace compiles and passes strict all-target/all-feature Clippy for
both `x86_64-pc-windows-msvc` and `x86_64-pc-windows-gnu`. MSVC DLL/import-library
linking uses cargo-xwin; GNU linking uses the installed MinGW GCC wrapper.
`exports-capture-print-msvc.json` and `exports-capture-print-gnu.json` record actual
PE artifacts with 70 of 72 public exports. Only the two pipe entry points are
absent. Export presence does not establish behavior compatibility.

Separate [Windows guest evidence](../native-windows-abi/README.md) compares 278
behavior/layout rows and 65 exports with the original DLL using the same MinGW
caller. The expanded actual guest layout probe matches all 44 measured Rust/C
constants. Native-produced WIMs independently verify and apply with the original
reader. [Printing](../native-windows-print/README.md) passes 42 exact raw-output
comparisons in Windows text/binary modes, including French locale and Unicode.
Capture's runtime and metadata evidence is tracked separately; full Windows
filesystem, NTFS and installation gates remain open.

Earlier artifact records remain preserved: `exports-before-xml.json` has 63
exports, and `exports.json`/`exports-gnu.json` have 65 after portable C-runtime XML
support. Their guest comparisons include XML allocation released by caller C
`free`, C `FILE` output and wide strings. Keep each frozen hash attached to its
own measured results.

The initial cross-check exposed 15 test type errors where narrow C strings were
passed to wide-character APIs. Shared test encoding now uses actual `TChar`
width; platform-specific helpers compile where their backends use them.

```sh
cargo clippy --manifest-path Cargo.toml --target-dir target --target x86_64-pc-windows-msvc --workspace --all-targets --all-features --locked -- -D warnings
cargo xwin build --manifest-path Cargo.toml --target-dir target --target x86_64-pc-windows-msvc --locked -p wim
python3 scripts/wimlib/audit-windows-exports.py target/x86_64-pc-windows-msvc/debug/wim.dll --output /tmp/windows-current-exports.json
```

The export audit deliberately exits nonzero while required names are absent.
The GNU Rust target requires both the MCF thread library search path and the
installed MinGW winpthreads search path (`libpthread.a`). The initial link attempt
with only the MCF path failed on that missing archive. The final GNU DLL uses
MSVCRT and Windows system libraries and does not require VCRUNTIME. Exact hashes
and imported DLLs are recorded in the guest evidence.
