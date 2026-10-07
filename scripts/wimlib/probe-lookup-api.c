/* Test-only client, compiled unchanged against both libraries. */
#include <wimlib.h>
#include <stddef.h>
#include <stdio.h>

static int list(const struct wimlib_resource_entry *e, void *ctx)
{
    unsigned *count = ctx;
    ++*count;
    printf("row:");
    for (unsigned i = 0; i < 20; ++i) printf("%02x", e->sha1_hash[i]);
    printf(":%llu:%llu:%llu:%u:%u:%u:%u:%u:%u:%u:%u:%llu:%llu:%llu:%llu\n",
        (unsigned long long)e->uncompressed_size,
        (unsigned long long)e->compressed_size, (unsigned long long)e->offset,
        e->part_number, e->reference_count, e->is_compressed, e->is_metadata,
        e->is_free, e->is_spanned, e->is_missing, e->packed,
        (unsigned long long)e->raw_resource_offset_in_wim,
        (unsigned long long)e->raw_resource_compressed_size,
        (unsigned long long)e->raw_resource_uncompressed_size,
        (unsigned long long)e->reserved[0]);
    return 0;
}
static int stop(const struct wimlib_resource_entry *entry, void *ctx)
{
    (void)entry;
    return *(int *)ctx;
}
int main(int argc, char **argv)
{
    WIMStruct *wim = NULL;
    printf("layout:%zu:%zu:%zu:%zu:%zu\n", sizeof(struct wimlib_resource_entry),
        offsetof(struct wimlib_resource_entry, sha1_hash),
        offsetof(struct wimlib_resource_entry, part_number),
        offsetof(struct wimlib_resource_entry, reference_count),
        offsetof(struct wimlib_resource_entry, raw_resource_offset_in_wim));
    if (argc != 2) return 2;
    int ret = wimlib_open_wim(argv[1], 0, &wim);
    printf("open:%d\n", ret);
    if (ret) return 3;
    unsigned count = 0;
    printf("iterate:%d\n", wimlib_iterate_lookup_table(wim, 0, list, &count));
    printf("count:%u\n", count);
    for (int code = -37; code <= 37; code += 74)
        printf("stop:%d\n", wimlib_iterate_lookup_table(wim, 0, stop, &code));
    printf("flags:%d\n", wimlib_iterate_lookup_table(NULL, 1, NULL, NULL));
    wimlib_free(wim);
    if (wimlib_create_new_wim(WIMLIB_COMPRESSION_TYPE_NONE, &wim)) return 4;
    count = 0;
    printf("empty:%d:%u\n", wimlib_iterate_lookup_table(wim, 0, NULL, NULL), count);
    wimlib_free(wim);
    return 0;
}
