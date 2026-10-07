# Native XML property model evidence

The native `wim-format::xml::XmlInfo` implementation translates the behavioral
contracts of `/tmp/wimlib/src/xml.c` and `xmlproc.c`. Its parser intentionally
implements the upstream dialect rather than generic XML: it accepts literal
colon element names and lenient names, supports only the five named entities,
rejects numeric character references, discards comments/processing instructions,
merges adjacent text/CDATA, preserves mixed content and unknown attributes, and
limits element nesting to 50 levels. It validates root WIM, encryption markers,
and unique contiguous one-based IMAGE indices independently of their document
order. WIM resources decode and encode UTF-16LE; serialization includes a BOM,
explicit closing tags and escaping of all five XML special characters.

Property methods support slash paths and bracketed sibling indices. Setting
None/empty deletes an element; setting text clears its attributes and all prior
children. Creation requires preceding sibling indices to exist, and failed
traversal preserves ancestors already created. Invalid-path removal succeeds as
an upstream no-op. NAME collision checks apply only to the exact path NAME,
not NAME[1], following the original public API. Name matching is case-sensitive.

## Reproduction

Run from the repository root:

```sh
cargo test --manifest-path Cargo.toml -p wim-format --test xml --locked
cargo build --manifest-path Cargo.toml -p wim-format --example xml_status --locked
python3 scripts/wimlib/check-xml-differential.py --oracle /tmp/wimlib-native-oracle/.libs/libwim.so --native /data/cache/rust/debug/examples/xml_status --fixture crates/wim-format/tests/fixtures/xpress-resource.wim
cargo clippy --manifest-path Cargo.toml -p wim-format --all-targets --locked -- -D warnings
```

Observed results: **6 Rust regression tests passed**, **39 differential XML and
public property cases passed**, all-target Clippy passed. The native path does
not call libwim. The independent Python harness uses original libwim public
`open_wim`, `get_image_property`, `set_image_property`, `create_new_wim`, and
`add_empty_image` as the oracle. For successful parsed documents it also compares
canonical serialization with the upstream test-support
`wimlib_parse_and_write_xml_doc`. Fresh WIM fixtures replace XML descriptors and
append new XML resources to an upstream-produced WIM; no native-generated
expected XML is substituted for the oracle.

The differential suite covers malformed root, image indices, encryption,
excessive nesting, unsupported entities, duplicate properties, CRLF text,
unknown attributes, namespaced-looking names, invalid XML names accepted by
upstream, UTF-16 supplementary characters, removal, flags, replacing children,
indexed sibling creation, path syntax/error precedence, name collision,
case sensitivity and the NAME[1] collision-check bypass. The trailing-slash
comparison discovered an initial mismatch, corrected with a regression test.

## Remaining scope

This module is the XML property layer, not the complete image-management API.
Add/delete/export image and synthesized count/timestamp updates need integration
with mutable archive/image state. The Rust API rejects embedded NUL property
values; C callers terminate at NUL and an eventual C ABI adapter must perform
that boundary conversion. Allocation exhaustion is not yet translated to
WIMLIB_ERR_NOMEM. Tests exercised Linux UTF-8 public APIs plus UTF-16LE resource
encoding; Windows `tchar` ABI behavior needs its platform gate. Differential
coverage is finite and does not establish full replacement compatibility.
