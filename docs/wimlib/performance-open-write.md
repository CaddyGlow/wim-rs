# File-backed opening and uncompressed writing

Measured on 2026-10-03 with the same C API caller and frozen libraries. The paired run uses 15 measured trials per library, one discarded warmup, alternating execution order, CPU 2 affinity, one compression worker, warm filesystem cache, and the retained 516-file synthetic input (approximately 18 MiB). Writes do not include fsync. Upstream is wimlib 1.14.5 built with `-O2`; its SSE4.2 path is disabled through `WIMLIB_DISABLE_CPU_FEATURES=sse4.2`, as in the earlier benchmark. The host was not reserved exclusively for these measurements.

## Paired uncompressed results

Medians; memory is whole-process peak RSS, including the caller and runtime.

| Metric | Upstream | Rust before | Rust after |
| --- | ---: | ---: | ---: |
| Capture + write | 35.97 ms | 43.81 ms | 38.40 ms |
| Open | 0.056 ms | 41.88 ms | 0.151 ms |
| Verify | 10.40 ms | 9.55 ms | 10.77 ms |
| Apply | 17.93 ms | 20.61 ms | 22.29 ms |
| Write peak RSS | 3.32 MiB | 22.15 MiB | 5.05 MiB |
| Read peak RSS | 2.99 MiB | 37.15 MiB | 5.06 MiB |

Opening is about 277 times faster than the previous Rust library. Capture/write improves 12.3%, leaving a 6.8% gap against upstream. Verify and apply each increase approximately 1–2 ms with bounded positional reads. Opening still has about 0.095 ms absolute overhead against upstream; these results do not establish parity.

The separate seven-trial run covers none, XPRESS, LZX, LZMS and solid LZMS. Rust opening medians are 0.151–0.160 ms across all five modes, replacing the earlier 12–42 ms payload-size-dependent opening. Compressed writer performance and memory are not the target of this change.

## Implementation and cause

Opening previously loaded the complete archive into owned buffers. Handles now retain a file and read the header, XML and lookup descriptors at open. Physical lookup descriptors are cached and shared; mutable handle reference counts retain a separate small descriptor copy. Metadata and payload consumers read bounded ranges, with independent logical positions over positional file reads. Exported and referenced handles retain the source file after the source handle is freed or its filename is removed.

Fresh uncompressed output now streams payloads directly to the output file using a reusable 32 KiB buffer. It avoids per-chunk allocation and accumulation of the entire WIM in an output vector. Integrity generation scans the written output with bounded scratch space. Captured emission retains the previous optimization that avoids hashing data twice. Progress, cancellation and completed-file behavior remain covered by existing tests.

Mutation/rewrite paths still request a lazy whole-file compatibility snapshot when they require byte slices. Compressed fresh output and compact paths retain their existing buffering. Append output now retains only the new tail, with absolute offsets into the prefix already on disk, and calculates integrity from the file. This fixed a 6.6 GB allocation failure while inserting WinRE into the real baseline install image; [the retained run](evidence/uup-pipeline-current-repaired-1004/README.md) records failure, correction and independent verification. These changes do not make every operation constant-memory.

## Validation

Every paired trial independently verifies the generated WIM with upstream and compares upstream-extracted content against the source. Reference extraction is also compared. All 45 measured trials and three warmups passed; source contents were preserved. The five-mode run likewise verified 70 measured operations and ten warmup operations. Library hashes and raw timings are retained in the evidence JSON.

- WIM Rust workspace: 395 passed, one ignored; strict all-target/all-feature Clippy and formatting passed.
- Upstream portable suites: all three suites passed for both upstream and Rust with an independent strict content comparator.
- Unchanged genuine upstream randomized caller: final Rust library passed a 60-second run.
- Large-file regression: an 8 GiB sparse WIM opens without a retained allocation exceeding 1 MiB; truncation subsequently reports error 65. Counting-reader tests verify metadata-only opening, bounded resource reads, integrity progress and cancellation.
- windows-uup host package: 304 passed, eight ignored. Root strict Clippy passed. The broader root test run has an unrelated concurrent cpcopy failure in `verbose_directory_merge_is_silent_and_creation_precedes_contents`.
- Windows x64 MSVC compile check passed. Windows runtime gates were not run.

The orphan-solid regression fixture was moved from the removed performance evidence directory into `wim/tests/fixtures/`; its provenance is retained alongside it.

## Reproduction and evidence

Use `scripts/wimlib/benchmark-open-write.py --before BEFORE_LIB --fixture-report RETAINED_REPORT --cpu 2 --repetitions 15 --output REPORT`. It preserves inputs, freezes libraries and callers, alternates ordering, and validates outputs outside measured timings. The retained report must point to an existing source tree and reference WIM from `benchmark-performance.py`.

[Paired timings](evidence/file-backed-open-write/paired.json), [all compression modes](evidence/file-backed-open-write/all-modes.json), [upstream suite results](evidence/file-backed-open-write/upstream.json), and [randomized caller result](evidence/file-backed-open-write/randomized.json). Full local logs are under `/data/cache/wim-open-write-baseline/`.

Final measured Rust library SHA-256: `2ce25f6a1277c0fd23cc79b63159853587fdf36cceee7fb521e4b3004a07f1c1`.
