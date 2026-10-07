# Native image selector resolution

`XmlInfo::resolve_image` supplies the safe UTF-8 selector operation needed by
future archive handles. It follows positive decimal index precedence, exact
image-name matching, ASCII case-insensitive `all`, `*`, leading C-locale
whitespace, signs, and overflow rejection. The regression test initially failed
to compile because the method was absent, then passed after implementation.

`differential.json` records 26 comparisons with the original public API on a
seven-image handle, including numeric names, signed names, a trailing-space
name, Unicode, and an overflowing numeric name that must not match by name.

Reproduce:

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim-format --example resolve_image
python3 scripts/wimlib/check-image-resolution.py --oracle /tmp/wimlib-native-oracle/.libs/libwim.so --native target/debug/examples/resolve_image
```

This is an internal API, not the `wimlib_resolve_image` C export. Windows
UTF-16 and process-locale character classification have not been validated.
