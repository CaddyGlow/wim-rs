# Native image selection and export evidence

`src/image_ops.rs` serializes selected images, reorders them, deletes images
(including the final image), and exports images into an existing destination.
No original implementation is called by production code. Original wimlib is
used only by `scripts/wimlib/check-image-operations.py` as the verifier and
filesystem extractor. The script captures a three-image fixture from distinct
source trees with one shared payload, a hardlink alias, and distinct payloads.

The recorded run covers seven selections, including the empty image set, plus
one export into an existing one-image destination. All eight archives pass the
original verifier. Fourteen extracted image trees match the source file bytes.
The script additionally checks exact retained payload hashes and reference
counts (two hardlink references per image), confirms unreferenced payloads are
pruned, asserts boot image index remapping, and confirms an opaque XML property
is retained. Four Rust tests cover native content and metadata preservation,
invalid indices, duplicate selection rejection, empty destination export, and
name collision without destination mutation.

The API is buffered and requires complete unsplit archives. It does not yet
implement C export flags, explicit name/description overrides, resource
reference contexts, in-place modification transactions, or public ABI exports.
Selected raw metadata and unknown XML fields survive unchanged; image INDEX
attributes are renumbered and root TOTALBYTES is updated for the new layout.
