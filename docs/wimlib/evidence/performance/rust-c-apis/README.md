# Matched Rust and C API benchmark

`scripts/wimlib/benchmark-apis.py` builds the `benchmark_apis` release example
and benchmarks both interfaces to the same native Rust engine. Capture, write,
open, verification, and extraction have matching options and timing boundaries.
Initialization is outside timing. C pathname conversion is inside timing, as it
is part of calling that interface. Peak RSS covers the whole probe process.

Each codec has a warmup and alternating API order. Both readers use one reference
archive. Each generated archive is verified and extracted by the opposite API,
and extracted file SHA-256 hashes must match the input. Hashing and optional
original-wimlib verification are outside timing. These checks compare file
contents, not full filesystem metadata. Reports retain samples, medians, ranges,
the frozen executable hash, input hashes, and working directories.

The retained 2026-10-05 runs used four 1 MiB files and 64 small files, three
measured repetitions per API, and one warmup. They are smoke validation of the
benchmark, not controlled evidence of a performance advantage; the successful
native and oracle smoke runs overlapped on the host.

- `results.json`: both APIs completed all four ordinary codecs, 24 measured pairs
  of write/read probes, with matching extracted contents.
- `oracle-results.json`: none, XPRESS, and LZX also passed original-wimlib
  verification for both APIs, including warmups.
- `oracle-lzms-failure.json`: the initial independent run failed when the
  original verifier received SIGSEGV on the C-interface LZMS warmup archive.
  The work directory and archive are retained in the report. This remains an
  unresolved independent LZMS verification limitation; native checks passed.

For a fresh comparison, run from the repository root:

```sh
python3 scripts/wimlib/benchmark-apis.py --output /tmp/wim-api-results.json
```

The default input is 18 MiB with five repetitions. An existing input directory
can be selected with `--source`; its contents are preserved. Solid compression
and original-versus-native C-library performance remain in the separate
`benchmark-performance.py` workload.
