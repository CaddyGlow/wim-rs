# Windows selected paths and NTFS streams

The Rust backend supports selected paths, ancestor preservation, flattened roots,
component globs and text path lists on Windows. Stream sinks identify both inode
and stream slot, so multiple streams sharing a content hash are restored to the
correct NTFS names. Hardlink aliases are restricted to selected entries. Empty
unknown placeholder records are ignored; nonempty unsupported streams fail.
Named data streams include empty streams and streams on directories and the
extraction root. Reparse streams are hash-checked and restored with
`FSCTL_SET_REPARSE_POINT`; absolute links captured with fixups are relocated to
the extraction target. Reparse payloads are limited to 16,376 bytes. Compression
and sparse controls are applied through native handles; zero sparse blocks are
skipped and final stream lengths are restored.

`tests.json` records three regression tests run in a disposable Windows 11 NTFS
VM using a statically linked Windows executable. They check wide filenames,
placeholder stream records, file/directory/root ADS, empty ADS, hardlinks,
selected unsupported metadata, flattening, globs and UTF-16 path lists.

`differential.json` compares the original Windows wimlib DLL and the Rust DLL
using the same unchanged public-header C caller and one WIM captured by the
original DLL from real NTFS files. Nine cases compare return values, content
hashes for all enumerated data streams, attributes, ordinary-file/directory
creation/write times and SDDL, and symlink/junction targets. Extraction target
prefixes are normalized for relocated links. The input WIM hash is checked
before and after. Reparse security/times and allocation ranges are not part of
this comparison; sparse allocation should not be inferred from attribute checks.

Reproduce after compiling the C probe against the original `wimlib.h` and
cross-compiling the Rust DLL with a static CRT:

```sh
python3 scripts/wimlib/check-windows-extract-streams.py \
  --qga-socket vm-state/<disposable-id>/qga.sock \
  --dll <Windows-Rust-wim.dll>
```

EFS, object IDs and extended attributes are covered by the newer
[NTFS metadata evidence](../windows-ntfs-metadata/README.md). UNIX tagged metadata,
stdout extraction and broader Windows flag/privilege behavior remain separate gates. This evidence
does not establish full UUP reconstruction, servicing or installed-target
correctness. Linux ordinary-filesystem extraction omits Windows ADS/security;
WIM-to-WIM export/repack preserves them, and direct Linux NTFS apply is absent.
