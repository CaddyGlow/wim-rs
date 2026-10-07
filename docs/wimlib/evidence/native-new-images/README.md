# Native new archive construction

`wim-format::image_build::ImageBuilder` builds an unsplit image set entirely
from `OwnedMetadata` trees and caller-supplied content bytes. It does not use
an input WIM, template, original-library call, or captured metadata fixture.
Caller-supplied GUID bytes are retained. Empty streams use a zero hash;
nonempty streams are interned by native SHA-1 and unreferenced content is
omitted. Lookup reference counts count each canonical inode stream once per
visible alias across every image, following original `write.c`
`inode_find_blobs_to_reference()` and its `i_nlink` weighting. Image XML
statistics follow `xml.c` `xml_update_image_info()` (directories include root,
total bytes include all streams and aliases, hardlink bytes exclude the first
alias). Custom property paths, names and descriptions use the native XML layer.

The current implementation serializes a raw intermediate image set and feeds
the existing native ordinary archive writer. Both complete archive copies,
metadata and content are buffered. This is not bounded streaming and does not
provide capture, filesystem interpretation, transactions, or public C exports.
XML string allocations inherit the existing XML layer's infallible allocation
behavior. The caller generates production GUIDs and timestamps; the evidence
producer uses an explicitly chosen deterministic test GUID and zero times.

Reproduce from the repository root:

```sh
cargo test --manifest-path Cargo.toml --target-dir target --locked -p wim-format --test image_build
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim-format --example create_image
python3 scripts/wimlib/check-new-image-differential.py --output docs/wimlib/evidence/native-new-images/original-differential.json
```

Three Rust integration tests cover wholly new image construction in all four
compression modes, metadata hardlink normalization, reference counts, XML
statistics/properties, explicit GUID and boot selection, unreferenced content
omission, zero-image archives, name collisions, invalid boot selection, and
missing stream content. The first integration run failed to compile until the
new module's public export was added; there is no behavioral red-run claim for
this orchestration layer. Its underlying serializers and codecs have their own
separate red/green evidence.

`original-differential.json` records 24 cases against the independently built
original wimlib 1.14.5 binary: four codecs, integrity enabled/disabled, and zero,
one or two images. The original verifies every archive and applies all 24
nonempty images. The comparator checks applied content for both hardlink aliases,
their shared inode number and an empty directory. Original detailed metadata
listing also checks the exact opaque null-DACL security descriptor, named stream
name and SHA-1, unnamed stream SHA-1, reference counts and hardlink count. Original
header inspection checks explicit GUID and boot index. Original verify checks
all referenced data including named streams.

Linux apply is requested with `--no-acls`: these cases establish metadata
preservation and original parser acceptance, not Windows ACL application, ADS
installation, NTFS integration, or Windows servicing correctness. This is a
new-image writer primitive, not yet `wimlib_add_image()` compatibility.
