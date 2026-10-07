/* Test-only unchanged-header verification client. */
#include <wimlib.h>
#include <stdio.h>
int main(int argc, char **argv) {
    WIMStruct *wim = NULL;
    if (argc != 2) return 2;
    int ret = wimlib_open_wim(argv[1], 0, &wim);
    printf("open:%d\n", ret);
    if (ret) return 0;
    printf("flags:%d\n", wimlib_verify_wim(wim, 1));
    printf("verify:%d\n", wimlib_verify_wim(wim, 0));
    printf("delete:%d\n", wimlib_delete_image(wim, WIMLIB_ALL_IMAGES));
    printf("after-delete:%d\n", wimlib_verify_wim(wim, 0));
    wimlib_free(wim);
    printf("null:%d\n", wimlib_verify_wim(NULL, 0));
    if (wimlib_create_new_wim(WIMLIB_COMPRESSION_TYPE_NONE, &wim)) return 3;
    printf("empty:%d\n", wimlib_verify_wim(wim, 0));
    printf("add-empty:%d\n", wimlib_add_empty_image(wim, NULL, NULL));
    printf("owned-empty:%d\n", wimlib_verify_wim(wim, 0));
    wimlib_free(wim);
    return 0;
}
