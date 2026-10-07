# Native global diagnostics and lifecycle

The unchanged upstream `wimlib.h` C client compares five exports against the independently built original 1.14.5 library: `wimlib_set_print_errors`, `wimlib_set_error_file`, `wimlib_set_error_file_by_name`, `wimlib_global_init`, and `wimlib_global_cleanup`.

`differential.json` records 130 matching fresh-process observations across all flag values 0–127, -1, and INT_MIN. Each checks valid/invalid/repeated initialization; real file-open and XML property error/warning bytes; stderr defaults; buffered stdout ordering; errno and unchanged text-loader outputs; suppression; failed sink replacement; borrowed FILE survival; append-mode named sinks; closure of owned descriptors on replacement and initialized cleanup; cleanup before initialization; repeated cleanup; and sink write failure through `/dev/full`. The client uses host C FILE objects and closes borrowed streams itself.

`syntax-message-red.json` preserves the original/native failure that exposed a missing XML path syntax diagnostic. Native property validation now emits that diagnostic, as well as illegal-name errors and illegal-value warnings. Text-file open/stat/read failures use the same sink. The setters therefore change observable behavior of actual APIs.

`tests.log` records passing wim-format and wim tests. `text-regression.json` records 2,440 matching text-loading observations after diagnostic integration. Native NTFS upcase mappings are reconstructed from the original compressed Windows uppercase table and drive directory case comparison. Exact matching names take precedence over folded names.

Reproduce from the repository root after building wim:

```sh
python3 scripts/wimlib/check-diagnostics-abi.py --native target/debug
```

The claim is Linux host behavior for these observations. Remaining gates include Windows privilege acquisition/global platform resources, Windows wide stdio/log sharing, other Unix errno implementations, custom allocator hooks/OOM injection, diagnostics from APIs not yet connected to this sink, and callback-backed FILE reentrancy. The FILE ownership mutex serializes replacement and writes; reentrant custom FILE callbacks are not validated. Ambiguous case-folded directory names without an exact match and non-Linux default comparison behavior require additional original-platform observations. This evidence does not promote all global runtime initialization effects or all upstream diagnostic paths to complete compatibility.

## Automatic initialization ordering

`auto-init.json` adds 78 matching fresh-process observations through create, open, and open-with-progress, including NULL/empty filenames, NULL output storage, invalid compression/open flags, missing files, untouched output pointers, and subsequent valid/conflicting/unknown global initialization flags. Create initializes before validating parameters; open initializes only after flags, filename, and output validation. Missing-file opens initialize even though opening fails, and now emit the original read-only-open diagnostic with preserved OS errno. `auto-init-red.json` retains the earlier output mismatch and also the original crash when diagnostics were enabled before initialization without installing a FILE sink. Final probes suppress diagnostics for uninitialized calls to avoid invoking that upstream NULL-FILE behavior.

`ntfs-upcase.json` compares all 65,536 code units (131,072 output bytes) against the unchanged original static library's `init_upcase` and `upcase`, using a test-only C client. All entries match. `ntfs-assumption-red.log` preserves the incorrect final-sigma test assumption and its failure. The corrected regression test preserves NTFS's identity mapping for final sigma rather than assuming modern Unicode uppercase rules. `ntfs-upcase-tests.log` and `clippy.log` record passing targeted tests and strict Clippy.

```sh
python3 scripts/wimlib/check-auto-init-abi.py --native target/debug
python3 scripts/wimlib/check-ntfs-upcase.py
```
