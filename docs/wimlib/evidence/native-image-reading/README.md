# Native image reading evidence

The workspace passes 128 tests (`rust-tests.log`), including replay of 9,443
upstream decoder observations. Formatting and all-target/all-feature Clippy with
warnings denied pass.

`archive-differential.json` records eleven original-C capture/native-Rust read
comparisons: uncompressed, XPRESS, LZX and LZMS ordinary/pipable resources plus
all three compressed solid layouts. Exact extracted payload bytes match. The
native reader also has fixture contracts composing XML and metadata reads.

[XML evidence](../xml-native.md) records 39 original-library comparisons.
[Metadata evidence](../../../../crates/wim-format/tests/METADATA-EVIDENCE.md)
records 40 comparisons. Integrity evidence lives beside the format tests. These are
seekable in-memory primitives. No native compressor, archive writer, filesystem
capture/apply, C ABI, Windows, FUSE or direct-NTFS gate is established here.

Reproduce archive comparison:

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked --example read_blob
python3 scripts/wimlib/check-archive-differential.py --oracle /tmp/wimlib-native-oracle/wimlib-imagex --native target/debug/examples/read_blob
```
