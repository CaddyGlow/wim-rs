# Actual Windows CRT printing

The unchanged-header MinGW/MSVCRT caller `probe-windows-print.c` runs against both
the original Windows DLL and the native GNU DLL in the owned disposable Windows
11 guest. `original.json`/`native.json` establish two basic modes; the exact raw
output bytes match (1,423 bytes in text mode, 2,768 bytes in binary mode).
`missing-export-red.json` preserves the initial native missing-symbol guard.

Text mode uses the real CRT's ANSI conversion and CRLF translation. With the
guest's CP1252 locale, the Unicode name stops after `Print-é` before the CJK
character, but the format's following newline is still printed. Binary mode emits
UTF-16LE without a BOM, including the complete surrogate pair and LF newlines.
The decoded `stdout` field is only a convenience; `stdout_base64` is authoritative.

`expanded-original.json`/`expanded-native.json` contain 42 exact cases: text and
binary output; image indices -2, -1, 0, 1 and 2; embedded newlines in names and
descriptions; Japanese language values; an unpaired UTF-16 surrogate; and C versus
`French_France.1252` locales. Every process exits 0. A fixed FILETIME represents
2020-02-03T04:05:06Z so French February formatting exercises real non-ASCII
`wcsftime` output. The installed French locale succeeds; no locale-unavailable
case is silently treated as success. The same executable bytes and same CRT are
used for both libraries, and the original WIM fixture is preserved.

Compile the caller with the MinGW toolchain in
`../native-windows-abi/original-build.json`, `-municode -Werror`, and the unchanged
original header. Run `check-windows-print-api.py --qga-socket <owned-socket>
--expanded --output <original-record>`, then supply the native GNU `--dll`,
`--implementation`, `--baseline <original-record>` and a separate `--output`.
No Windows capture, extraction, servicing or installation behavior is inferred
from printing.

`expanded-original.json` and `expanded-native.json` pass 42/42 exact raw-byte
comparisons. Both modes cover image selectors -2/-1/0/1/2, names containing
newlines or unpaired surrogates, Japanese language properties, and C/French
locales with a fixed February 2020 timestamp. The installed French locale is
actually exercised; it is not skipped. Native Windows timestamp formatting now
uses the original wide C `wcsftime` path.
