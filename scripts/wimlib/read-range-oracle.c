/* Private-resource oracle, test only. Links the preserved original static build. */
#include <wimlib.h>
#include "wimlib/wim.h"
#include "wimlib/blob_table.h"
#include "wimlib/resource.h"
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
int main(int argc, char **argv) {
    if (argc != 6) return 2;
    WIMStruct *wim = NULL;
    int error = wimlib_open_wim(argv[1], 0, &wim);
    if (error) return error;
    uint8_t hash[20];
    for (size_t i = 0; i < 20; i++) {
        unsigned value;
        if (sscanf(argv[2] + i * 2, "%2x", &value) != 1) return 2;
        hash[i] = value;
    }
    struct blob_descriptor *blob = lookup_blob(wim->blob_table, hash);
    if (!blob) return WIMLIB_ERR_RESOURCE_NOT_FOUND;
    uint64_t offset = strtoull(argv[3], NULL, 10);
    size_t size = strtoull(argv[4], NULL, 10);
    void *bytes = malloc(size ? size : 1);
    if (!bytes) return WIMLIB_ERR_NOMEM;
    error = read_partial_wim_blob_into_buf(blob, offset, size, bytes);
    if (!error) {
        FILE *file = fopen(argv[5], "wb");
        if (!file || fwrite(bytes, 1, size, file) != size) return 2;
        fclose(file);
    }
    free(bytes);
    wimlib_free(wim);
    return error;
}
