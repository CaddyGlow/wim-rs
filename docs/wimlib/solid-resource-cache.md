# Solid resource reads in reconstruction

Real UUP reconstruction exposed repeated decoding of a solid ESD chunk for each
contained file and each 32 KiB writer range. The writer also reparsed the source
lookup table for each selected blob. These costs do not appear in small ordinary
WIM fixtures.

Each retained input now shares one decoded solid chunk across its file-backed
archive views. Reads validate framing, reread compressed bytes and compare them
before reusing decoded output. A changed input forces decoding again; truncated
input and failed decodes cannot be hidden by the cache. Corruption-recovery
extraction bypasses caching. Ordinary and pipable reads retain their previous
behavior. The cache holds one compressed chunk and one decoded chunk per input;
it does not retain the entire decompressed archive.

Writer blobs retain the source file view alongside existing byte snapshots used
by append/compaction and raw-resource copying. Origin queries use the retained
header, and decoded ranges use the retained lookup and chunk cache. This does
not remove every writer snapshot or establish a whole-process memory bound.

`wim-format/tests/file_resource.rs` checks reuse across selections, eviction on
another chunk, changed input, failed decode and truncation. Native-produced
ordinary, pipable and solid fixtures check selected ranges against the independent
buffered reader. The `wim-format` and `wim` host suites and strict Clippy pass.

The interrupted verification and export attempts are retained in
`evidence/uup-pipeline-20261004/` and
`evidence/uup-pipeline-20261004-cached/`. Their interrupted timings are diagnostic
evidence, not a paired performance benchmark. The final real-media gate is
recorded separately in `evidence/uup-pipeline-20261004-final/`.
