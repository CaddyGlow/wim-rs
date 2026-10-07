/* Public mutation history: real inline/deferred hashes, deletion and reinsert. */
#include <wimlib.h>
#include <stdio.h>
static int row(const struct wimlib_resource_entry *e, void *ctx)
{
    (void)ctx;
    printf("row:");
    for (unsigned i = 0; i < 20; i++) printf("%02x", e->sha1_hash[i]);
    printf(":%llu:%u:%u\n", (unsigned long long)e->uncompressed_size,
           e->reference_count, e->is_metadata);
    return 0;
}
static void dump(WIMStruct *wim, const char *label)
{
    printf("%s:%d\n", label, wimlib_iterate_lookup_table(wim, 0, row, NULL));
}
int main(int argc, char **argv)
{
    WIMStruct *source = NULL, *destination = NULL;
    if (argc != 2) return 2;
    printf("create:%d\n", wimlib_create_new_wim(0, &source));
    printf("add:%d\n", wimlib_add_image(source, argv[1], "capture", NULL, WIMLIB_ADD_FLAG_NO_ACLS));
    printf("destination:%d\n", wimlib_create_new_wim(0, &destination));
    printf("gift:%d\n", wimlib_export_image(source, 1, destination, NULL, NULL, WIMLIB_EXPORT_FLAG_GIFT));
    dump(source, "source"); dump(destination, "destination");
    printf("sourceverify:%d\n", wimlib_verify_wim(source, 0));
    wimlib_free(source);
    printf("destinationverify:%d\n", wimlib_verify_wim(destination, 0));
    wimlib_free(destination);
    return 0;
}
