/* Compile against unchanged original header; no-FUSE build precedence and state. */
#include <wimlib.h>
#include <stdio.h>
#include <errno.h>
#include <string.h>
static int callbacks;
static enum wimlib_progress_status progress(enum wimlib_progress_msg msg,
        union wimlib_progress_info *info, void *context)
{
    (void)msg; (void)info; ++callbacks;
    return (enum wimlib_progress_status)*(int *)context;
}
int main(int argc, char **argv)
{
    if (argc != 3) return 2;
    WIMStruct *created = NULL, *opened = NULL;
    if (wimlib_create_new_wim(0, &created) || wimlib_add_empty_image(created, "Pending", NULL)
        || wimlib_open_wim(argv[1], 0, &opened)) return 3;
    WIMStruct *handles[] = {NULL, created, opened};
    const char *dirs[] = {NULL, "", argv[2], "/tmp/native-wim-no-such-mountpoint"};
    const char *staging[] = {NULL, "", argv[2]};
    const int flags[] = {0, 1, 2, 4, 8, 16, 32, 64, 127, 128, -1};
    for (unsigned h = 0; h < 3; ++h)
        for (int image = -1; image <= 2; ++image)
            for (unsigned d = 0; d < 4; ++d)
                for (unsigned f = 0; f < 11; ++f)
                    for (unsigned s = 0; s < 3; ++s) {
                        errno = 123;
                        int ret = wimlib_mount_image(handles[h], image, dirs[d], flags[f], staging[s]);
                        printf("mount %u %d %u %d %u ret %d errno %d\n", h, image, d, flags[f], s, ret, errno);
                    }
    for (unsigned d = 0; d < 4; ++d)
        for (unsigned f = 0; f < 11; ++f) {
            errno = 123;
            int ret = wimlib_unmount_image(dirs[d], flags[f]);
            printf("unmount %u %d ret %d errno %d\n", d, flags[f], ret, errno);
            for (int status = 0; status < 3; ++status) {
                callbacks = 0; errno = 123;
                ret = wimlib_unmount_image_with_progress(dirs[d], flags[f], progress, &status);
                printf("unmount-progress %u %d %d ret %d errno %d callbacks %d\n", d, flags[f], status, ret, errno, callbacks);
            }
        }
    wimlib_set_print_errors(true);
    errno = 123;
    int diagnostic = wimlib_mount_image(NULL, 0, NULL, -1, NULL);
    printf("diagnostic ret %d errno %d\n", diagnostic, errno);
    wimlib_set_print_errors(false);
    struct wimlib_wim_info info;
    if (wimlib_get_wim_info(created, &info)) return 4;
    printf("created images %u boot %u\n", info.image_count, info.boot_index);
    if (wimlib_get_wim_info(opened, &info)) return 5;
    printf("opened images %u boot %u\n", info.image_count, info.boot_index);
    wimlib_free(opened); wimlib_free(created);
    return 0;
}
