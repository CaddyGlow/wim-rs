#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv)
{
    if (argc < 4 || (argc - 4) % 2) return 2;
    WIMStruct *wim = NULL;
    int result = strcmp(argv[1], "new") == 0 || strcmp(argv[1], "empty") == 0
        ? wimlib_create_new_wim(atoi(argv[2]), &wim)
        : wimlib_open_wim(argv[1], 0, &wim);
    if (result) { printf("open:%d\n", result); return 0; }
    if (!strcmp(argv[1], "empty")) wimlib_add_empty_image(wim, "pending", NULL);
    struct wimlib_wim_info info;
    memset(&info, 0, sizeof(info));
    for (unsigned i = 0; i < sizeof(info.guid); i++) info.guid[i] = i;
    wimlib_set_wim_info(wim, &info, WIMLIB_CHANGE_GUID);
    for (int i = 4; i < argc; i += 2)
        wimlib_set_image_property(wim, 1, argv[i], argv[i + 1]);
    printf("prefix:");
    wimlib_print_header(wim);
    printf("images:");
    wimlib_print_available_images(wim, atoi(argv[3]));
    wimlib_free(wim);
    return 0;
}
