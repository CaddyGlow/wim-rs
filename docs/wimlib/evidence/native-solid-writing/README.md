# Native solid archive serialization evidence

`wim_format::solid_write::write_solid_archive` writes version 0xe00 WIMs
with one solid data resource, zero-hash marker lookup entry, contiguous logical
blob offsets and retained reference counts. Image metadata remains ordinary;
source GUID, XML properties and boot metadata are retained. Optional integrity covers
bytes 208 through the end of the lookup table. Input payload hashes, image counts,
metadata parsing and stream references are validated by the ordinary writer.

The original source oracle is `/tmp/wimlib-native-oracle/wimlib-imagex`, built
from wimlib 1.14.5 commit `cd5e231c348c255ae5088873b5a66ee0eb96fa07`.
Production Rust does not invoke or link it.

`red.log` records two failing behavioral tests against the Unsupported stub.
`green.log` records three passing tests: the matrix of three source layouts,
three output compressors and two integrity modes, invalid compression/split
rejection, and boot metadata preservation/missing-content rejection.

`differential.json` records 66 independently verified outputs. The C oracle
captures 11 source combinations (None/XPRESS/LZX/LZMS with ordinary/pipable/solid,
excluding None solid), appends a second boot image, and extracts a reference tree.
Rust writes XPRESS/LZX/LZMS solid archives with and without integrity. Original C
`verify` accepts every output; C `apply` of both images preserves file contents,
Unicode names, empty files, symlink targets, hardlink groups, and Unix modes. The
script checks boot index 2 and records each output's SHA-256. No writer byte-for-byte
identity or compression-ratio parity is claimed.

Reproduction:

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim-format --example solid_repack
python3 scripts/wimlib/check-solid-write-differential.py --oracle /tmp/wimlib-native-oracle/wimlib-imagex --native target/debug/examples/solid_repack
cargo test --manifest-path Cargo.toml --target-dir target --locked -p wim-format --test solid_write
```

Limits: this primitive buffers the ordinary staging archive, concatenated payload,
compressed resource and final output, and writes one solid resource. It is not
bounded-memory streaming, transactional overwrite, multipart output or the public
wimlib write API. The C ABI and platform-specific operations require separate gates.

Current writers refresh final root TOTALBYTES; preliminary pipable XML omits it. The later [XML evidence](../xml-writing/README.md) records updated contracts and rerun reader matrices. Historical output hashes here precede that correction.
