/* SPDX-License-Identifier: LGPL-2.1-or-later
 * Test-only structure layout probe. Build separately with -DCODEC=1, 2, or 3,
 * -DHAVE_CONFIG_H, upstream source/config include directories, and
 * -ffunction-sections -fdata-sections -Wl,--gc-sections. */
#include <stdio.h>
#include <stddef.h>
#if CODEC == 1
#include "xpress_compress.c"
int main(void) {
 printf("xpress %zu %zu %zu %zu %zu\n",
 offsetof(struct xpress_compressor, hc_mf), offsetof(struct xpress_compressor, bt_mf),
 sizeof(struct hc_matchfinder), sizeof(struct bt_matchfinder), sizeof(struct lz_match));
}
#elif CODEC == 2
#include "lzx_compress.c"
int main(void) {
 printf("lzx %zu %zu %zu %zu %zu %zu %zu %zu\n",
 offsetof(struct lzx_compressor, hc_mf_16), offsetof(struct lzx_compressor, hc_mf_32),
 offsetof(struct lzx_compressor, bt_mf_16), offsetof(struct lzx_compressor, bt_mf_32),
 sizeof(struct hc_matchfinder_16), sizeof(struct hc_matchfinder_32),
 sizeof(struct bt_matchfinder_16), sizeof(struct bt_matchfinder_32));
}
#elif CODEC == 3
#include "lzms_compress.c"
int main(void) { printf("lzms %zu\n", sizeof(struct lzms_compressor)); }
#else
#error "CODEC must be 1, 2 or 3"
#endif
