# Native C text-file loading: partial implementation

`wimlib_load_text_file` is implemented independently of WIM handles. It reads files using their initial advertised byte size, or consumes the host C stdin FILE when the path is NULL or exactly `-`. It returns a separately allocated, terminated platform string that the unchanged-header client releases with C `free`. The returned count includes embedded NULs and excludes the appended terminator. Neither newline conversion nor line parsing is added on Linux.

Encoding detection follows original `textfile.c` in order: UTF-16LE BOM, initial ASCII byte followed by NUL, UTF-8 BOM, then the default UTF-8 mode. On Linux the UTF-8 mode is a lossless identity copy, including malformed UTF-8 bytes. UTF-16LE converts with upstream WTF-8 surrogate handling; odd byte lengths fail with error 30 and errno EILSEQ. UTF-16BE and UTF-32 are not additional supported formats; their bytes follow the original detection rules.

`differential.json` proves exact original/native output for **2,440 unchanged-header C client cases**, including the return code, errno, output publication, complete text bytes, character count and terminator. Every successful allocation is released by the client using its normal C `free`. Cases cover:

- BOM-only/empty buffers; ASCII, UTF-8 BOM and UTF-16LE heuristic detection; mixed CR/LF; embedded NUL; malformed UTF-8; truncated UTF-16; WTF-8 and noncharacters.
- Every nonzero and zero single-byte input; each of the 2,048 individual surrogate units; and one UTF-16LE file containing the complete range of 65,536 code units, independently converted by both libraries.
- NULL and `-` stdin selectors, growth boundaries at 255/256/257 and 768/769 bytes, and 1 MiB inputs. C `ungetc` pushback verifies that already buffered stdin bytes are consumed correctly.
- Empty/missing/unreadable paths, a literal dash filename reached through its full path, a raw non-UTF-8 filename, directories, closed stdin, and a proc file whose advertised size is zero.
- Deterministic fstat failure, read failure and unexpected EOF using a test-only Linux LD_PRELOAD shim. These return errors 63, 50 and 65 with matching errno, while preserving both output parameters.

`contract-red.log` records the missing native export. `encoding-errno-red.json`, `stat-errno-red.json`, and `empty-path-red.json` record real differential failures that were corrected: conversion must set EILSEQ; stat errors must restore the underlying I/O errno after Rust cleanup; and this API passes an empty path to open rather than applying the WIM-open API's invalid-parameter rule. The native reader also preserves upstream's EINVAL on unexpected EOF.

Two translation tests and two ABI ownership/error tests pass. Their output is saved in `translation-tests.log` and `ownership-tests.log`. `clippy.log` records strict Clippy validation. The differential runner freezes the native cdylib in a temporary directory before launching clients so concurrent Cargo linking cannot remove the library during an observation; its SHA-256 is recorded in the evidence.

Reproduce:

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim --lib
python3 scripts/wimlib/check-text-file-abi.py --native target/debug
cargo test --manifest-path Cargo.toml --target-dir target --locked -p wim --test text_file
cargo test --manifest-path Cargo.toml --target-dir target --locked -p wim --lib text_file::tests
```

The export remains **partial**. Registered allocator hooks and exhaustive allocation-failure behavior are not integrated: some intermediate Rust allocations can abort rather than return the original error. Windows wide strings, stdin text-mode translation, matching CRT ownership and errno need original Windows-library validation. The host C stdin symbol and errno handling have been proven only on Linux, not every Unix ABI. Outputs must be valid writable pointers; upstream does not define NULL output pointers as a supported invocation.
