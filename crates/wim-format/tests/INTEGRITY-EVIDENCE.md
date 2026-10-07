# Native integrity verification evidence

The implementation follows `/tmp/wimlib/src/integrity.c`: table fields are three little-endian u32 values (total size, entry count, chunk size), followed by 20-byte SHA-1 digests. Coverage begins at byte 208 and ends immediately after the blob table. The XML document, integrity table itself, and WIM header are not covered. Table readers accept any nonzero chunk size; writers normally choose 10 MiB.

The checked-in upstream-generated fixture is recalculated byte-for-byte by the Rust implementation, then verified. Regression cases exercise covered corruption, excluded header/XML corruption, empty coverage, partial and full final chunks, malformed size/count/zero chunk/coverage, every truncation of the table, digest mismatch before later truncation, callback ordering, initial and post-chunk cancellation, absent resources by zero offset, and invalid/overflowed blob table bounds.

Independent upstream oracle run uses `ctypes` to call `wimlib_open_wim(path, WIMLIB_OPEN_FLAG_CHECK_INTEGRITY=1, ...)` in `/tmp/wimlib-native-oracle/.libs/libwim.so`. Observed return codes:

| Case | Upstream result |
| --- | --- |
| Original fixture | 0 |
| Four-entry 128-byte chunks, final 13-byte chunk | 0 |
| Byte 208 flipped | 13 (integrity failure) |
| GUID byte 24 flipped | 0 |
| Table size changed to 12 | 19 |
| Table entry count changed to 2 | 19 |
| Table chunk size changed to 0 | 19 |
| Table chunk size changed to 1 | 19 |
| Final table byte truncated | 65 |

The Rust low-level result preserves upstream's distinction between a checksum mismatch and an error; an eventual ABI layer must map `IntegrityStatus::Mismatch` to public error 13 when opening with integrity checking. `IntegrityTable::verify_with_progress` returns callback errors unchanged.

Validation: `cargo test --manifest-path Cargo.toml -p wim-format --test integrity`: eight tests passed. This establishes seekable byte-slice checking and table calculation, not native file I/O, incremental old-table checksum reuse, pipe streaming, resource writing, or ABI behavior.
