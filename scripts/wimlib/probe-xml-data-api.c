/* Test-only unchanged-header raw XML and host stdio client. */
#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>

static void read_xml(WIMStruct *wim, const char *label) {
    void *buffer = (void *)(uintptr_t)1;
    size_t size = 123;
    int result = wimlib_get_xml_data(wim, &buffer, &size);
    printf("%s:%d:%zu:", label, result, size);
    if (result == 0) {
        const unsigned char *data = buffer;
        for (size_t i = 0; i < size; ++i) printf("%02x", data[i]);
        free(buffer);
    } else printf("unchanged=%u", buffer == (void *)(uintptr_t)1);
    puts("");
}
int main(int argc, char **argv) {
    WIMStruct *wim = NULL;
    if (argc != 2) return 2;
    int result = wimlib_open_wim(argv[1], 0, &wim);
    printf("open:%d\n", result);
    if (result) return 0;
    read_xml(wim, "raw");
    size_t size = 123;
    printf("null-buffer:%d\n", wimlib_get_xml_data(wim, NULL, &size));
    printf("unchanged-size:%zu\n", size);
    void *buffer = NULL;
    printf("null-size:%d\n", wimlib_get_xml_data(wim, &buffer, NULL));
    printf("edit:%d\n", wimlib_set_image_name(wim, 1, "changed"));
    read_xml(wim, "after-edit");
    FILE *fp = tmpfile();
    if (!fp) return 3;
    if (fwrite("pre", 1, 3, fp) != 3) return 4;
    printf("extract:%d\n", wimlib_extract_xml_data(wim, fp));
    printf("position:%ld\n", ftell(fp));
    rewind(fp);
    printf("stdio:");
    int byte;
    while ((byte = fgetc(fp)) != EOF) printf("%02x", (unsigned)byte);
    puts("");
    fclose(fp);
    fp = fopen("/dev/null", "r");
    if (!fp) return 5;
    printf("write-error:%d\n", wimlib_extract_xml_data(wim, fp));
    fclose(fp);
    wimlib_free(wim);
    if (wimlib_create_new_wim(WIMLIB_COMPRESSION_TYPE_NONE, &wim)) return 6;
    read_xml(wim, "new");
    printf("new-null:%d\n", wimlib_get_xml_data(wim, NULL, NULL));
    wimlib_free(wim);
    return 0;
}
