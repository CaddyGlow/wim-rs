# Local Linux library package

The native Linux cdylib now has the original ELF ABI name `libwim.so.15`, derived
from upstream libtool `42:0:27`. Cargo still produces `libwim.so`; the local
packager copies it as `libwim.so.15` and provides the development symlink,
unchanged public header, license text, status documentation and a hash manifest.

`smoke.json` records an actual original CLI loading this package and reporting
the matching library version. No system installation or publication occurred.
The three portable suites have separate behavioral evidence under
[`native-full-upstream`](../native-full-upstream/README.md). The SONAME and
package files do not establish the remaining allocator, FUSE, NTFS, platform or
complete ABI gates.

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim
python3 scripts/wimlib/package-native-linux.py
```

The packager audits actual exports and SONAME before writing a fresh local
destination. It preserves existing package directories. Use `--library` for a
reviewed release artifact or `--output` for a fresh chosen directory.
