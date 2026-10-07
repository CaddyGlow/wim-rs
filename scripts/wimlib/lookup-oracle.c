/* Test-only public C API lookup projection. */
#include <inttypes.h>
#include <stdio.h>
#include <wimlib.h>

static int print_entry(const struct wimlib_resource_entry *entry, void *context)
{
    (void)context;
    unsigned flags = entry->is_free | (entry->is_metadata << 1) |
                     (entry->is_compressed << 2) | (entry->is_spanned << 3) |
                     (entry->packed << 4);
    printf("ENTRY ");
    for (unsigned i = 0; i < 20; i++) printf("%02x", entry->sha1_hash[i]);
    printf(" %" PRIu64 " %u %u %" PRIu64 " %" PRIu64 " %" PRIu64 " %" PRIu64 "\n",
           entry->uncompressed_size, entry->reference_count, flags, entry->offset,
           entry->raw_resource_offset_in_wim, entry->raw_resource_compressed_size,
           entry->raw_resource_uncompressed_size);
    return 0;
}

int main(int argc, char **argv)
{
    if (argc != 2) return 2;
    WIMStruct *wim = NULL;
    int status = wimlib_open_wim(argv[1], 0, &wim);
    if (status) {
        printf("STATUS %d\n", status);
        return 0;
    }
    struct wimlib_wim_info info;
    status = wimlib_get_wim_info(wim, &info);
    if (!status) {
        printf("STATUS 0 %u\n", info.image_count);
        status = wimlib_iterate_lookup_table(wim, 0, print_entry, NULL);
    }
    wimlib_free(wim);
    return status ? 1 : 0;
}
