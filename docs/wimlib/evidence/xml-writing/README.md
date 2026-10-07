# XML image operations and writer statistics

Native XML helpers select/append one-based image lists, retain unknown root and
image properties, renumber INDEX, and update/remove top-level TOTALBYTES without
invalidating cached image positions. Two contracts cover those helpers; two
writer contracts cover final stored-byte statistics in all layouts and absent
statistics in preliminary pipable XML. Both helper and writer regression red
logs are preserved. All-ones numeric values retain the original error-sentinel
behavior (read as zero), while their text remains representable.

Writers now refresh root TOTALBYTES to the stored offset immediately after the
blob table, matching upstream `write.c:finish_write` and `xml.c` preparation.
Pipable preliminary XML omits that root element, matching
`write_pipable_wim`; final XML contains the new value. Image properties and
unknown XML data remain semantically preserved through native serialization.
Exact original XML bytes are no longer a writer invariant.

`original-policy-differential.json` records eleven original/native writer layout
comparisons across all codecs. Each original and native output independently
reports its own actual lookup-table end, and all four preliminary pipable XML
copies omit the root statistic. Compression outputs need not be identical, so
their byte totals are compared to their own offsets rather than to each other.

The ordinary (88), solid (66), and pipable (88) original-reader verify/apply
matrices were rerun after the change; the pipable matrix includes fragmented
stdin applications. Results are preserved in the reread JSON files here.
`workspace-tests.log` records 210 passing tests, including image operations,
split/join and file-backed resource reading. Strict Clippy and formatting pass.

Reproduce:

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked --examples
python3 scripts/wimlib/check-xml-writing.py --oracle /tmp/wimlib-native-oracle/wimlib-imagex --native-dir target/debug/examples
```

The public USE_EXISTING_TOTALBYTES flag and XML allocation-failure parity remain
unimplemented. Helper cloning uses the existing XML representation's allocation
behavior. No C export is promoted by these internal functions.
