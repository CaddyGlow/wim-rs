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
static void hash(WIMStruct *wim)
{
    WIMStruct *destination = NULL;
    printf("hashcreate:%d\n", wimlib_create_new_wim(0, &destination));
    printf("hash:%d\n", wimlib_export_image(wim, 1, destination, NULL, NULL, 0));
    wimlib_free(destination);
}
int main(int argc, char **argv)
{
    WIMStruct *wim = NULL;
    if (argc != 2) return 2;
    printf("create:%d\n", wimlib_create_new_wim(0, &wim));
    printf("add:%d\n", wimlib_add_image(wim, argv[1], "capture", NULL, WIMLIB_ADD_FLAG_NO_ACLS));
    dump(wim, "inline");
    hash(wim); dump(wim, "hashed");
    printf("rename:%d\n", wimlib_rename_path(wim, 1, "/file", "/renamed"));
    dump(wim, "renamed");
    printf("unlink:%d\n", wimlib_delete_path(wim, 1, "/link", 0));
    dump(wim, "unlinked");
    printf("delete:%d\n", wimlib_delete_image(wim, 1));
    dump(wim, "empty");
    printf("readd:%d\n", wimlib_add_image(wim, argv[1], "capture", NULL, WIMLIB_ADD_FLAG_NO_ACLS));
    hash(wim); dump(wim, "reinserted");
    wimlib_free(wim);
    return 0;
}
