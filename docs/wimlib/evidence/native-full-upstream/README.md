The genuine upstream CLI ELF from `/tmp/wimlib-native-oracle/.libs/wimlib-imagex` runs with a frozen native `libwim.so.15` selected through `LD_LIBRARY_PATH`. The CLI is neither reimplemented nor rebuilt against native code. The upstream portable scripts are copied unchanged into separate disposable build trees; `srcdir=/tmp/wimlib/tests` resolves their source files and helpers without modifying source inputs. The independent tree comparator is compiled from the original `tests/tree-cmp.c` using the original configured header.

All three portable scripts completed with status 0 against the original library first, then against native:

- `test-imagex`: ordinary capture/apply, hardlinks, names, XML/header inspection, integrity, append/delete, multi-image application, split/join and export/recompression.
- `test-imagex-capture_and_apply`: upstream common capture trees, split/join/export, pipable writes and stdin application, configurations and source-list overlays.
- `test-imagex-update_and_extract`: update/delete/rename/add and selected extraction behavior.

`results.json` records exact frozen CLI/library/script SHA-256 values, statuses and retained logs. Native library SHA-256 is `9eb32063b9e41563c8d5339bcd919ccd1b825ca134dd594e4a63bce899a77ae3`. The original scripts contain expected failing operations; their error messages in logs do not mean the overall script failed. No first-suite failure was discarded: these first runs completed successfully.

The upstream capture/apply harness has a weakness: `do_tree_cmp()` reports a comparator failure only inside an `/usr/bin/tree` availability condition. The unmodified first-run evidence remains intact. The separate `strict-comparator/` run changes only that function in disposable script copies so a nonzero comparator result always fails. It preserves the original comparator and all actual capture/apply cases. All three original-library scripts passed this strengthened run before all three native-library scripts also passed. Executed script hashes distinguish the strengthened copy from the unchanged source. Source scripts were never edited.

Reproduce from the repository root:

```sh
python3 scripts/wimlib/check-full-upstream.py
python3 scripts/wimlib/check-full-upstream.py --strict-comparator --output docs/wimlib/evidence/native-full-upstream/strict-comparator
```

The runner freezes actual input libraries before execution, retains disposable work directories and all logs, and stops at the first failing script per library. Original baseline failure prevents a native run. `WIMLIB_DISABLE_CPU_FEATURES=sse4.2` avoids the preserved original LZMS filter CPU crash. No production dependency on original C code is introduced.

This is proof for the three portable scripts on this Linux host, rather than the entire upstream test suite or Windows correctness. Mount, NTFS and Windows scripts are excluded because their platform/tooling requirements differ. The tests do not establish complete allocation-failure, callback-reentrancy, compression-quality or cross-platform parity; the API ledger retains partial classifications.
