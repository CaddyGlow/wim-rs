# Path mutation TDD contract

`original-contract.json` records 387 original C observations for
`wimlib_delete_path` and `wimlib_rename_path`. The same unchanged-header C client
will be linked to the native library by `check-path-mutation.py --native ...`.
`contract-red.log` records the native linker failure for both absent exports.
Native implementations now match all 387 in-memory cases. `differential.json`
records stdout comparisons, and `diagnostics.json` additionally records exact
stderr diagnostics with reporting enabled. The implementations stage an owned
tree and commit metadata/reference changes only after successful serialization.
The strengthened write comparison now also passes for this matrix; this is
partial host evidence rather than a completed drop-in claim.

The matrix exercises file, directory, root, missing and intermediate-file paths;
NULL and empty paths; forward/backslash canonicalization; invalid images and
delete flags; forced and recursive deletion; replacement of files and empty
directories; ancestry loops; hardlink aliases; and both shared exported owners
before and after releasing the other handle.
Every call records errno, result and the complete remaining directory listing,
so error returns alone do not satisfy the contract. Inputs remain untouched;
each client opens an independent handle and makes only in-memory changes.

The implementation must follow `update_image.c` rather than host filesystem
rename semantics. Command paths are copied/canonicalized after selecting the
image and checking its metadata owner count. Delete FORCE suppresses a missing
path, while deleting directories requires RECURSIVE. Rename resolves source,
destination and destination parent, checks types/nonempty targets and ancestry,
then journals unlink/name/link changes. Failure must roll back all tree and
resource-reference changes; mapped WIM errors and Unix errno are both observable.

Per-image ownership tokens now survive export/re-export and are released on
image deletion and handle release. `owner-red.log` preserves the missing-state
regression, and `owner-green.log` records its passing lifecycle test. Failed
export does not retain an extra owner. The mutation APIs now use those tokens
to return error 86, and both shared owners before/after release match original C.
Pending-root shared state
alone cannot prove that rule for original images. Preserve inode aliases, streams, security and
opaque Windows metadata during tree edits. Compact detached nodes before owned
serialization, and update blob counts per live dentry; retaining backing blobs
at zero references follows the existing image-deletion policy.

The first write checks passed 124 original archive verifications and 62 paired
original-reader applications with identical contents, modes, xattrs, symlinks
and hardlink partitions. Extending the comparator to post-write XML statistics
exposed a real failure retained in `write-interop-red.log`: deleting one hardlink
alias leaves native FILECOUNT/TOTALBYTES/HARDLINKBYTES stale. Dirty-image identity
tracking propagates through export and is removed on deletion. The writer now
refreshes statistics for dirty images before opening the output, leaving clean
image properties alone. The fresh `write-interop.json` records 387 matching
cases, 124 original verifications, 62 equal original-reader application pairs,
and equal post-write counts. `write-interop-red.log` retains the pre-fix failure.

The eleven `ordinary-*`, `solid-*` and `pipable-*` matrices extend the same
contract across every supported codec/layout combination (solid excludes none).
They record 4,257 matching cases, 1,364 original verifications and 682 equal
application pairs. Each records the SHA-256 of the frozen native artifact.
`unicode-sensitive.json` and `unicode-insensitive.json` add 432 cases each,
388 verifications and 194 equal application pairs, including supplementary
characters, accented names and distinct Greek sigma forms.

`collation-red.log` preserves the mismatch that exposed raw UTF-16 child sorting.
Original `dentry.c` collates names using the NTFS uppercase table first, then
original code units to break ties. Metadata reading and owned serialization now
use that comparator; lookup still prefers an exact name before a folded match.
The expanded comparisons include callback listing order and written XML counts.
Filesystem snapshots compare contents, modes, xattrs, symlinks and hardlink
partitions; timestamp/ownership and Windows security parity need further gates.

Green validation must extend these observations with written archive verification
and extraction by the original reader, C callback traversals after successful
and failed edits, raw WTF-8 names, named streams, shared-owner release, and
allocator failure at each staging/commit step. Current internal tree allocations
do not yet route through the C allocation hooks. Multi-command update transactions,
capture ADD operations, Windows behavior and concurrent/reentrant mutation
remain separate required gates. Passing this initial matrix is not full update
or drop-in compatibility.
