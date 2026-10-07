/* Real independent captured descriptors, unchanged source public header. */
#include <wimlib.h>
#include <stdio.h>
#include <string.h>
static int row(const struct wimlib_resource_entry *e, void *ctx)
{
    (void)ctx;
    printf("row:");
    for (unsigned i = 0; i < 20; i++) printf("%02x", e->sha1_hash[i]);
    printf(":%llu:%u:%u\n", (unsigned long long)e->uncompressed_size,
           e->reference_count, e->is_metadata);
    return 0;
}
int main(int argc, char **argv)
{
    WIMStruct *source = NULL, *hashed = NULL, *dest = NULL;
    if (argc != 4 && argc != 6) return 2;
    printf("create:%d\n", wimlib_create_new_wim(0, &source));
    printf("add:%d\n", wimlib_add_image(source, argv[1], "capture", NULL, WIMLIB_ADD_FLAG_NO_ACLS));
    printf("hashdest:%d\n", wimlib_create_new_wim(0, &hashed));
    /* Source export actually hashes the deferred file descriptors. */
    printf("hash:%d\n", wimlib_export_image(source, 1, hashed, NULL, NULL, 0));
    WIMStruct *metadata = NULL;
    if (argc == 6) {
        printf("stagewrite:%d\n", wimlib_write(hashed, argv[4], WIMLIB_ALL_IMAGES, 0, 1));
        printf("metadata:%d\n", wimlib_open_wim(argv[4], 0, &metadata));
    }
    wimlib_free(hashed);
    printf("dest:%d\n", wimlib_create_new_wim(0, &dest));
    printf("reference:%d\n", wimlib_reference_resources(dest, &source, 1, 0));
    printf("lookup:%d\n", wimlib_iterate_lookup_table(dest, 0, row, NULL));
    if (metadata) {
        printf("image:%d\n", wimlib_export_image(metadata, 1, dest, NULL, NULL, 0));
        wimlib_free(metadata);
    }
    wimlib_free(source);
    printf("verify:%d\n", wimlib_verify_wim(dest, 0));
    if (argc == 6) printf("write:%d\n", wimlib_write(dest, argv[5], WIMLIB_ALL_IMAGES, 0, 1));
    FILE *f = fopen(argv[2], "wb");
    if (!f) return 3;
    fputs("mutated payload same length", f); fclose(f);
    printf("changed:%d\n", wimlib_verify_wim(dest, 0));
    if (argc == 6) printf("changedwrite:%d\n", wimlib_write(dest, argv[5], WIMLIB_ALL_IMAGES, 0, 1));
    f = fopen(argv[2], "wb");
    if (!f) return 3;
    fclose(f);
    printf("truncated:%d\n", wimlib_verify_wim(dest, 0));
    if (argc == 6) printf("truncatedwrite:%d\n", wimlib_write(dest, argv[5], WIMLIB_ALL_IMAGES, 0, 1));
    f = fopen(argv[2], "wb");
    if (!f) return 3;
    fputs(argv[3], f); fclose(f);
    printf("retry:%d\n", wimlib_verify_wim(dest, 0));
    if (argc == 6) printf("retrywrite:%d\n", wimlib_write(dest, argv[5], WIMLIB_ALL_IMAGES, 0, 1));
    wimlib_free(dest);
    return 0;
}
