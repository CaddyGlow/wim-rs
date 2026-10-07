# Windows host metadata capture validation

The Windows scanner enumerates `FileStreamInformation` on its existing no-follow
backup handle. It no longer reopens a pathname to enumerate streams, which could
follow a reparse target or reject a protected file. Stream record parsing checks
bounds, sizes, offsets, UTF-16 names, duplicates, and WIM stream-count limits;
query buffers grow from 4 KiB to a bounded 16 MiB. Deferred file and ADS reads also
use backup and no-follow semantics. Reparse data occupies slot zero; link-owned
named and nonempty unnamed data streams occupy extra slots. The writer serializes reparse payloads and link-owned data in their canonical
WIM slots. Extraction attaches the reparse point before restoring nonempty unnamed
data, temporarily spooling existing data to disk to keep memory bounded. Windows
regressions exercise both native extraction and independent DISM apply. EFS metadata handles
are closed before raw export. Unnamed stream placement uses the effective reparse
attribute after provider translation, avoiding duplicate main streams when a
dedup reparse is translated into ordinary data.

`CaptureOptions::strict_security` rejects security descriptor fallback. The
Windows caller must acquire backup, restore, and security privileges; the probes
require successful strict privilege acquisition. The input must remain unchanged
until deferred writing finishes. These changes concern Windows host capture, not
native offline NTFS or QCOW2 parsing.

Directory-tree reporting now indexes pending bindings and shared reference counts
once per traversal. Pending named streams report their actual sizes before write.
Captured-resource lookup groups by digest or unhashed shared ownership with an
index, preserving insertion order and source-selection semantics. These changes
remove repeated whole-graph scans that made real Windows metadata checks slow.
Host regressions cover named sizes, alias references, and equal-content independent
files before hashing. Case-sensitive directories are rejected because this engine
cannot restore their per-directory lookup flag. The no-follow directory query
uses Microsoft's [FileCaseSensitiveInformation](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/ne-wdm-_file_information_class)
(class 71); unsupported older OS/filesystem queries are distinguished from read
errors. A Windows negative test checks the rejection leaves no added image.

## Retained checks

| Evidence | Coverage | Result |
| --- | --- | --- |
| `rust-test.json` | Static CRT MSVC Rust regression: 160 ADS, empty/Unicode/unpaired-UTF-16/root/directory ADS, hard links, symlink-owned named and nonempty unnamed data, LZX write, verify, native and DISM apply; case-sensitive directory rejection | Two ignored Windows tests passed |
| `metadata-matrix.json` | Frozen VSS fixture captured with original and Rust DLLs, each applied by original, Rust, and DISM | Six cases passed under the explicit policy below; source preserved |
| `windows-tree.json` | Full frozen Windows directory: 157,160 entries compared through the original public C header | Exact metadata manifests match |
| `windows-volume.json` | Windows-directory WIM: 157,120 entries and 20.3 GB logical data; independent upstream resource verification and DISM integrity-checked apply | Passed; five representative DLL, driver, and registry payload inventories match exactly |
| `windows-applied-tree.json` | All 157,120 applied entries compared with the frozen source | No missing or extra paths; 189 differences satisfy the explicit DISM transformations below |
| `protected-directory.json` | Directory denying list access to Everyone, scanned with enabled backup privileges | Original and Rust scans include the protected child and identical metadata |
| `ntfs-metadata.json` | EFS file/directory/empty file/hard link, binary EAs, object IDs and collision behavior, selected EFS extraction | Eight upstream/Rust differential cases passed |

The matrix independently reads NTFS stream names, sizes and SHA-256 payloads;
creation, last-write and last-access timestamps; owner/group/DACL/SACL with audit
ACEs and mandatory integrity labels; DOS names; attributes; raw/normalized reparse
semantics; and hard-link relationships. It includes protected backup reads,
relative/absolute/dangling/external symlinks, junctions with their own ADS, sparse
holes, NTFS compression and WOF LZX logical data. More than 4 KiB of stream records
exercises query-buffer growth. The fixture uses a VSS source and disables automatic
last-access updates in the disposable guest so inspection does not mutate results.

Original and Rust apply match the independently inventoried source exactly for
the recorded fields. DISM produces the same two differences for both captures:
it omits redundant short aliases equal to an 8.3 reparse filename, and represents
a null SACL as absent on the protected file. The raw differences remain in the
JSON. The script accepts only these explicit transformations reproduced by the
upstream capture; other differences, missing paths, or extra paths fail. This is
not a claim of byte-identical DISM metadata. Audit ACEs and mandatory integrity
labels themselves must match exactly.

The full applied Windows tree also records DISM setting the archive attribute
and replacing the NORMAL sentinel, changing SACL presence/inheritance status
bits, and representing empty SACLs as null. Owner, group, DACL, and nonempty SACL
contents remain identical. All other reported node fields, stream metadata, and
hard-link counts match. The comparison retains all 189 raw differences and fails
on transformations outside this explicit policy. Resource verification checks
every archived payload; the five applied sample inventories additionally check
stream SHA-256 values.

Each record identifies its own binary hashes. The full WIM write used an earlier
same-day DLL; subsequent indexing, case-sensitive-directory rejection, and
symlink-owned unnamed-data fixes have their separate regression evidence. It is
not a full-volume write certification of the final binary.

## Reproduction

Build the static CRT DLL and regression executable:

```sh
RUSTFLAGS='-C target-feature=+crt-static' nix develop --command cargo xwin build \
  --locked -p wim --target x86_64-pc-windows-msvc
RUSTFLAGS='-C target-feature=+crt-static' nix develop --command cargo xwin test \
  --locked -p wim --target x86_64-pc-windows-msvc \
  --test windows_capture --no-run
```

Build `scripts/wimlib/probe-windows-capture.c` and
`probe-windows-extract-paths.c` with MinGW and the pinned original wimlib header.
The original DLL is the independently built oracle identified in
`../native-windows-abi/original-build.json`. Artifact hashes are retained in each
JSON. Run the Rust test executable with `--ignored --nocapture` in an elevated
NTFS guest. Run the differential scripts against that explicitly owned guest:

```sh
python3 scripts/wimlib/check-windows-metadata-reliability.py \
  --qga-socket vm-state/<owned-guest>/qga.sock \
  --dll <static-crt-wim.dll> --probe-dir <compiled-c-probes> \
  --output <metadata-matrix.json>
python3 scripts/wimlib/check-windows-ntfs-metadata.py \
  --qga-socket vm-state/<owned-guest>/qga.sock \
  --dll <static-crt-wim.dll> --probe-dir <compiled-c-probes> \
  --output <ntfs-metadata.json>
```

Build `probe-windows-capture.c` with `-DCAPTURE_BUFFERED_OUTPUT` for large trees.
Use the retained fixture snapshot for a complete Windows-directory comparison:

```sh
python3 scripts/wimlib/check-windows-real-tree.py \
  --qga-socket vm-state/<owned-guest>/qga.sock \
  --fixture-evidence <metadata-matrix.json> --dll <static-crt-wim.dll> \
  --probe <buffered-capture-probe.exe> --output <windows-tree.json>
```

Build `probe-windows-capture-volume.c` with the original header for a full write
and upstream resource verification. `check-windows-volume-capture.py` accepts the
same fixture evidence, DLL, `--probe`, and `--output` options, plus `--apply` for
DISM apply with integrity checking and extended attributes. This captures the
Windows directory, not the complete system volume or an installable image.

After apply, compare samples and every entry with:

```sh
python3 scripts/wimlib/check-windows-applied-tree.py \
  --qga-socket vm-state/<owned-guest>/qga.sock \
  --volume-evidence <windows-volume.json> --probe <buffered-capture-probe.exe> \
  --output <windows-applied-tree.json>
```

The first script changes last-access policy and creates a VSS snapshot and fixture
inside the guest; use a disposable VM. Host wim tests and workspace Clippy passed;
Windows GNU all-target/all-feature Clippy and MSVC builds passed.

## Remaining gates

These results establish the recorded WIM-representable features on the selected
Windows/NTFS guest. They do not establish every reparse provider, detached WOF or
dedup backing, cloud placeholders, preservation of case-sensitive directories, offline
attribute-list/runlist parsing, or a bootable installed Windows image. Whole-system-volume
capture/write/apply and installation gates remain required before calling the
QCOW2 conversion reliable. WIM does not preserve every NTFS on-disk property,
such as physical allocation layout or the NTFS change timestamp.
