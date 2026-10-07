# Native WIM information and output settings

Six native C exports now implement `wimlib_get_wim_info`, `wimlib_set_wim_info`,
`wimlib_set_output_compression_type`, `wimlib_set_output_pack_compression_type`,
`wimlib_set_output_chunk_size` and `wimlib_set_output_pack_chunk_size`.

`contract-red.log` records the Rust integration contract failing before these
exports existed. `contract-green.log` records three passing tests for the 88-byte
information layout, masked edits and validation-before-mutation, output chunk
repair/reset behavior, unchanged input information after changing output
settings, and integrity presence determined by descriptor offset rather than
size. Rust fields represent the original ten contiguous bitfields as a u32.

`scripts/wimlib/probe-wim-info-api.c` includes the unchanged original public
header. Separate executables linked to the original C oracle and native library
produce exactly matching `original.log` and `native.log`: 608 masked header
mutation cases and complete 88-byte information structures, 672 compression and
chunk setter return values, and five file-backed information queries covering
ordinary metadata, integrity, XPRESS, pipable and solid archives. Newly allocated
random GUIDs are normalized by the actual GUID setter before comparison.
Unknown change masks and invalid boot indices leave the header unchanged.

Reproduce after building `wim`:

```sh
cc -Wall -Wextra -Werror -I /tmp/wimlib/include scripts/wimlib/probe-wim-info-api.c -L /tmp/wimlib-native-oracle/.libs -Wl,-rpath,/tmp/wimlib-native-oracle/.libs -lwim -o /tmp/wim-info-original
cc -Wall -Wextra -Werror -I /tmp/wimlib/include scripts/wimlib/probe-wim-info-api.c -L target/debug -Wl,-rpath,/home/rick/projects-caddy/windows-uup/target/debug -lwim -o /tmp/wim-info-native
/tmp/wim-info-original /tmp/metadata-native.wim /tmp/wim-integrity-valid.wim /tmp/wim-resource-xpress.wim /tmp/wim-resource-pipable.wim /tmp/wim-resource-solid.wim > /tmp/wim-info-original.log
/tmp/wim-info-native /tmp/metadata-native.wim /tmp/wim-integrity-valid.wim /tmp/wim-resource-xpress.wim /tmp/wim-resource-pipable.wim /tmp/wim-resource-solid.wim > /tmp/wim-info-native.log
diff -u /tmp/wim-info-original.log /tmp/wim-info-native.log
```

These exports are candidate implementations. The handle currently buffers the
input file, and its full opening/global initialization contracts remain under
review. Native output setters update stored settings exactly, but consumption
by C writers now has evidence in the adjacent native-ffi-write directory;
overwrite consumption still requires implementation and verification. C bitfield
layout has been executed on the current Linux x86-64 ABI; Windows and other
architectures remain gates. Filesystem readonly reporting uses the handle's
platform access helper and requires additional non-root permission/ACL tests.
No original C code is linked into the production native library.
