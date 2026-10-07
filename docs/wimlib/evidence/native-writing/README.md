# Native ordinary archive writing evidence

The byte-buffer archive serializer writes ordinary WIMs with raw, native XPRESS,
native LZX, or native LZMS resources. It retains every resolved image, the source GUID,
XML properties, metadata bytes, reference counts, boot-image
selection, and the readonly/reparse-fix header flags. Source payload hashes and
metadata structure are checked. Missing content and split input are rejected.
Optional integrity is recalculated over the new resource/table byte range.

`repack-red.log` preserves the initial uncompressed serializer's failing stub
contract. Three final serializer contracts exercise native round trips over
three input layouts and four output codecs, integrity present/absent, missing
content, split rejection, and boot-resource selection/normalization. The
compressed integration and error tests were added after the initial raw writer;
they do not have separate recorded red-phase logs.

`repack-differential.json` records 88 original-writer/native-writer/original-reader
cases. Eleven source codec/layout combinations are rewritten to four ordinary
output codecs with integrity present or absent. Each contains two images with
image two bootable. Original `wimlib-imagex verify` succeeds and original apply
of both images reproduces exact file bytes, Unicode paths, empty files, Unix
modes, hardlink groups and symlink targets. This fixture does not establish
Windows ACL, reparse-fix, alternate stream, or metadata-normalization parity.

The workspace test log records 178 passing tests including the XPRESS expanded-output
and capacity compatibility contracts. Strict all-target/all-feature Clippy
passes. See also the resource serializer, XPRESS encoder, and LZX/LZMS encoder
evidence directories for their independent low-level comparisons.

Reproduce:

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked --example repack
python3 scripts/wimlib/check-repack-differential.py --oracle /tmp/wimlib-native-oracle/wimlib-imagex --native target/debug/examples/repack
cargo test --manifest-path Cargo.toml --target-dir target --locked
```

This is a native internal writer primitive, not public `wimlib_write` parity.
Solid/pipable archive serialization, large-file bounded I/O,
image mutations, automatic XML size-stat updates, incremental overwrite,
callbacks/cancellation, write failure recovery and ABI integration remain.
No production dependency links the original C implementation.

Current writer behavior refreshes root TOTALBYTES. The later [XML evidence](../xml-writing/README.md) records the regression and rerun reader matrix; earlier output hashes here describe the prior preserved-stat policy.
