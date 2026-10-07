# Template-image checksum reuse

`link-red.json` preserves the initial missing native symbol.
`original-contracts.json` and `differential.json` record 576 exact unchanged-header
C comparisons of flags, null/same handles, image selectors, clean loaded images,
empty dirty images, and actual deferred captures. Lookup callbacks observe the
real copied template checksums on matching capture streams. Frozen-library hashes
are retained in each record.

The native graph compares exact paths, creation/write times, nondecreasing access
times, named-stream presence and stream sizes, then copies available template
hashes to the actual deferred stream bindings without reading source payloads.
When a matching hash already exists in the destination's stored resources, the
native graph now releases the deferred binding and transfers its references to
the existing resource. Otherwise it retains a real deferred source owner; copying
a checksum from a different handle alone does not copy that handle's resource.

`incremental-red.json` preserves the initial source-path/reference-count failure;
`alias-red.json` preserves the wrong hardlink representative-path selection.
`incremental-differential.json` records 16 exact original comparisons with actual
single-handle incremental backups (loaded template image 1, captured image 2),
cross-handle controls, hardlinks, template path exclusions, and deletion of
captured regular source files after referencing the template. The original CLI
verifies and applies each successful output, and the applied payloads and
hardlink topology match the captured source. Template input bytes remain intact.
Timestamp fields from independent captures are normalized in filesystem
comparisons; resource checksums, counts and API statuses remain exact.

Same-handle unchanged files remain writable after their capture paths disappear,
because the retained WIM resource owns the content. Cross-handle cases without a
stored destination resource still fail with error 47 after source deletion.
Hardlinks use the last captured dentry, following original `inode_any_dentry`
alias-head semantics; an absent representative path leaves hashes unresolved and
records ENOENT even if another alias exists in the template. Captured owners with
real equal hashes are canonicalized without payload reads.

Required gates still include allocation-failure parity, changed metadata and
named-stream matrices, graph edits which alter alias order, all resource kinds
and SHA-1 collision behavior, and platform metadata policies. No complete
incremental-backup claim is made.

```sh
python3 scripts/wimlib/check-template-image-api.py
python3 scripts/wimlib/check-template-incremental-api.py
```
