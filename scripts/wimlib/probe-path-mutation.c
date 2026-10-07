#include <wimlib.h>
#include <errno.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int entry(const struct wimlib_dir_entry *d, void *context)
{
    (void)context;
    printf("entry:%s:%" PRIu32 ":%" PRIu64 "\n", d->full_path,
           d->attributes, d->streams[0].resource.uncompressed_size);
    return 0;
}

int main(int argc, char **argv)
{
    if (argc != 8 && argc != 9) return 2;
    if (getenv("WIM_MUTATION_DIAGNOSTICS")) wimlib_set_print_errors(true);
    if (getenv("WIM_MUTATION_IGNORE_CASE")) wimlib_global_init(WIMLIB_INIT_FLAG_DEFAULT_CASE_INSENSITIVE);
    WIMStruct *wim = NULL;
    int result = wimlib_open_wim(argv[1], 0, &wim);
    if (result) { printf("open:%d\n", result); return 0; }
    WIMStruct *destination = NULL;
    int image = atoi(argv[3]);
    if (strcmp(argv[7], "single")) {
        result = wimlib_create_new_wim(0, &destination);
        if (!result) result = wimlib_export_image(wim, 1, destination, NULL, NULL, 0);
        printf("share:%d\n", result);
        if (!strcmp(argv[7], "dest-shared") || !strcmp(argv[7], "dest-released")) {
            WIMStruct *temporary = wim;
            wim = destination;
            destination = temporary;
        }
        if (!strcmp(argv[7], "dest-released") || !strcmp(argv[7], "source-released")) {
            wimlib_free(destination);
            destination = NULL;
        }
    }
    errno = 0;
    if (!strcmp(argv[2], "delete"))
        result = wimlib_delete_path(wim, image,
            !strcmp(argv[4], "@NULL") ? NULL : argv[4], atoi(argv[6]));
    else
        result = wimlib_rename_path(wim, image,
            !strcmp(argv[4], "@NULL") ? NULL : argv[4],
            !strcmp(argv[5], "@NULL") ? NULL : argv[5]);
    printf("result:%d:errno:%d\n", result, errno);
    result = wimlib_iterate_dir_tree(wim, 1, "/", WIMLIB_ITERATE_DIR_TREE_FLAG_RECURSIVE,
                                   entry, NULL);
    printf("tree:%d\n", result);
    if (argc == 9) {
        printf("write:%d\n", wimlib_write(wim, argv[8], WIMLIB_ALL_IMAGES, 0, 1));
        const char *properties[] = { "DIRCOUNT", "FILECOUNT", "TOTALBYTES", "HARDLINKBYTES" };
        for (unsigned i = 0; i < sizeof(properties) / sizeof(properties[0]); i++) {
            const char *value = wimlib_get_image_property(wim, 1, properties[i]);
            printf("property:%s:%s\n", properties[i], value ? value : "@NULL");
        }
    }
    wimlib_free(destination);
    wimlib_free(wim);
    return 0;
}
