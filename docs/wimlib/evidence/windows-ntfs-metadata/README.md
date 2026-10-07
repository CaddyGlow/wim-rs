# Windows WIM EFS, object IDs and extended attributes

Validated on 2026-10-04 in a disposable Windows 11 NTFS VM. Windows capture and
extraction preserve EFS raw ciphertext, object IDs and modern tagged binary EAs.
EFS raw export includes its named streams and recovery metadata, with no
plaintext conversion. Raw data is staged in delete-on-close temporary files;
transfer memory does not grow with encrypted-file size. Staged data is checked
against the WIM SHA-1 before import. Native metadata failures are returned to
the caller; object-ID uniqueness collisions follow the original wimlib policy.

`differential.json` records eight passing comparisons against original Windows
wimlib through unchanged public-header C callers. Four combinations capture and
apply with each backend. Two cases check object-ID collisions, and two check
selected EFS extraction. Independently read inventories compare exact EA bytes
(including binary values and FILE_NEED_EA), all 64 object-ID bytes, plaintext
hashes, raw EFS hashes, creation/write times, attributes and ordinary SDDL.
Fixtures include file/directory/root EAs, encrypted files with EAs, encrypted
directories with EAs and encrypted children, an empty encrypted file, and
hard-linked encrypted files. Fixture IDs are deliberately released after capture
so they can be restored elsewhere on the same NTFS volume; final destination IDs
remain allocated during the explicit collision cases. Input WIM hashes are
unchanged after all extraction runs. DLL and C caller hashes are recorded.

`tests.json` records four passing Windows regressions. The new EFS regression
checks encrypted data, encrypted ADS and hardlink aliases both directly from a
captured handle and after serialization/reopening. It verifies unchanged source
contents and deletion of all owned EFS spools after owners are released. The
selection regression confirms flattened extraction omits image-root ADS and
creation-time metadata when that root was not selected. Two host codec tests
cover binary EA round trips and malformed names, flags, lengths and offsets.

`streams-regression.json` records nine passing original-DLL comparisons for
existing ADS, sparse/compressed files, reparse points, fixups and selections.
Reparse target prefixes are normalized. Creation/write times of a newly created,
unselected flattened target root are excluded because each independent run
creates it at a different time; ordinary selected metadata remains compared.
Reparse security/times and sparse allocation ranges are not covered here.

`final-differential.json` repeats all eight NTFS comparisons after the resource
cache, writer lookup and empty-directory serialization fixes, with no differences.
The [real UUP evidence](../uup-pipeline-20261004-final/README.md) includes five final
Windows regressions and a DISM empty-directory mount check alongside reconstruction
and one audited SSU persistence check. Complete update closure and installed-target
correctness remain separate gates.

Reproduce using statically linked Windows DLL/test executables and C callers
compiled against the original wimlib public header:

```sh
python3 scripts/wimlib/check-windows-ntfs-metadata.py \
  --qga-socket vm-state/<disposable-id>/qga.sock --dll <Windows-Rust-wim.dll>
python3 scripts/wimlib/check-windows-extract-streams.py \
  --qga-socket vm-state/<disposable-id>/qga.sock --dll <Windows-Rust-wim.dll>
cargo test --locked --manifest-path Cargo.toml \
  -p wim --test windows_extract -- --include-ignored --test-threads=1
```

The EFS regression needs EFS-enabled NTFS and an account allowed to create/use an
EFS certificate. Its fixture uses the Windows PUBLIC directory because Windows
forbids EFS encryption in the SYSTEM temporary directory. Object IDs cannot be
duplicated on one NTFS volume: colliding IDs are skipped, as in original wimlib.
Other native metadata failures remain errors. Ordinary named-stream capture
outside EFS, deprecated Linux xattrs and broader Windows privilege/flag behavior
remain separate gates. This evidence does not establish full reconstruction,
DISM servicing or installed-target correctness, nor a Linux direct-NTFS backend.
