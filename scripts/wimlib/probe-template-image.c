#include <wimlib.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int resource(const struct wimlib_resource_entry *entry, void *context)
{
    (void)context;
    if (entry->is_metadata) return 0;
    printf("blob %llu %u ", (unsigned long long)entry->uncompressed_size,
           entry->reference_count);
    for (int i = 0; i < 20; i++) printf("%02x", entry->sha1_hash[i]);
    putchar('\n');
    return 0;
}

int main(int argc, char **argv)
{
    if (argc != 8) return 2;
    WIMStruct *destination = NULL, *source = NULL;
    int ret = wimlib_create_new_wim(0, &destination);
    if (ret) return 3;
    ret = wimlib_open_wim(argv[1], 0, &source);
    if (ret) return 4;
    int same_handle = getenv("TEMPLATE_SAME_HANDLE") != NULL;
    if (same_handle) {
        wimlib_free(destination);
        ret = wimlib_open_wim(argv[1], 0, &destination);
        if (!ret) ret = wimlib_add_image(destination, argv[2], "new", NULL, WIMLIB_ADD_FLAG_NO_ACLS);
    } else if (strcmp(argv[2], "empty") == 0)
        ret = wimlib_add_empty_image(destination, "new", NULL);
    else if (strcmp(argv[2], "clean") == 0) {
        wimlib_free(destination);
        ret = wimlib_open_wim(argv[1], 0, &destination);
    } else
        ret = wimlib_add_image(destination, argv[2], "new", NULL, WIMLIB_ADD_FLAG_NO_ACLS);
    printf("prepare %d\n", ret);
    if (!ret) {
        int mode = atoi(argv[6]);
        WIMStruct *new_wim = mode == 1 ? NULL : destination;
        WIMStruct *template_wim = mode == 2 ? NULL : mode == 3 || same_handle ? destination : source;
        errno = 123;
        ret = wimlib_reference_template_image(new_wim, atoi(argv[3]), template_wim,
                                              atoi(argv[4]), atoi(argv[5]));
        printf("template %d errno %d\n", ret, errno);
        printf("lookup %d\n", wimlib_iterate_lookup_table(destination, 0, resource, NULL));
        const char *remove_file = getenv("TEMPLATE_UNLINK_FILE");
        const char *remove_alias = getenv("TEMPLATE_UNLINK_ALIAS");
        if (!ret && remove_file) unlink(remove_file);
        if (!ret && remove_alias) unlink(remove_alias);
        if (strcmp(argv[7], "skip") != 0)
            printf("write %d\n", wimlib_write(destination, argv[7], WIMLIB_ALL_IMAGES, 0, 1));
    }
    wimlib_free(source);
    wimlib_free(destination);
    return 0;
}
