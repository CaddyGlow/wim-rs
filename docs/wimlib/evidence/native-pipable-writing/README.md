# Native pipable archive serialization evidence

The native `write_pipable_archive` serializer emits both the sequential portion
and the seekable trailer of unsplit pipable WIMs. It retains the source GUID,
images, metadata, XML properties, boot selection, and payload digests. Output framing
follows `/tmp/wimlib/src/write.c` (`write_pipable_wim`, `write_pwm_blob_header`,
`finish_write`) at commit `cd5e231c348c255ae5088873b5a66ee0eb96fa07`.
The original library is used only for test capture/verification/application.
Production code uses Rust encoders and SHA-1 and does not link to libwim.

## Validation

`archive-differential.json` records 88 cases: eleven original source codec/layout
combinations, four native output codecs (none, XPRESS, LZX, LZMS), and integrity
on/off. Each original source has two images and boot image 2. Every case passed
original `wimlib-imagex verify`, original seekable application of both images,
and original stdin pipe application of both images. The pipe producer uses
repeated fragments of 1, 7, 39, 207, and 4093 bytes and flushes after every
fragment. Linux pipes may coalesce fragments; this tests real unseekable input
without claiming specific read boundaries inside the C library.

Application comparisons cover Unicode filenames, nested directories, file
contents, empty files, symlink targets, permissions, and hardlink equivalence
classes. All payloads and metadata are first validated by the native reader.
Integrity is validated through the seekable original reader; pipe consumers
ignore the seekable trailer and therefore do not validate its integrity table.

`wim-format/tests/pipable_write.rs` includes three Rust regression tests covering
all output codecs and optional integrity, preliminary header omissions,
metadata-before-payload ordering, first XML framing, boot resource references,
and split/missing-content rejection. The initial TDD failure was a missing
`pipable_write` public module (compile error); after implementation, all tests
pass. This is an interface-first red test, not a claimed runtime failure.

Reproduce:

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim-format --example repack_pipable
python3 scripts/wimlib/check-pipable-write-differential.py --oracle /tmp/wimlib-native-oracle/wimlib-imagex --native target/debug/examples/repack_pipable
cargo test --manifest-path Cargo.toml --target-dir target --locked -p wim-format --test pipable_write
```

## Scope remaining

The serializer buffers resources and the complete result. It does not implement
bounded-memory streaming, output file descriptors, progress callbacks,
transactions, image filtering, split output, or the public C write API. The historical hashes precede the later XML statistic correction. No Windows,
FUSE, NTFS capture/application, or Microsoft reader compatibility is established
by these Linux tests; pipable WIM is an intentional wimlib extension.

Current writers refresh final root TOTALBYTES; preliminary pipable XML omits it. The later [XML evidence](../xml-writing/README.md) records updated contracts and rerun reader matrices. Historical output hashes here precede that correction.
