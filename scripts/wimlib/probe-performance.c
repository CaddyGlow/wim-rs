/* Same unchanged public header and caller for original and native libraries. */
#include "wimlib.h"
#include <stdio.h>
#include <stdlib.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <time.h>

static double now(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec / 1e9;
}

static void check(int status)
{
    if (status) {
        fprintf(stderr, "wimlib error %d: %s\n", status,
                wimlib_get_error_string(status));
        exit(1);
    }
}

static long peak_rss(void)
{
    /* ru_maxrss can retain the Python launcher's pre-exec high-water mark.
     * VmHWM measures this process's current address space after exec. */
    FILE *status = fopen("/proc/self/status", "r");
    if (!status) exit(2);
    char line[256];
    long value = -1;
    while (fgets(line, sizeof(line), status)) {
        if (sscanf(line, "VmHWM: %ld kB", &value) == 1) break;
    }
    fclose(status);
    if (value < 0) exit(2);
    return value;
}

int main(int argc, char **argv)
{
    if (argc != 6) return 2;
    WIMStruct *wim = NULL;
    double start, capture = 0, write = 0, open = 0, verify = 0, apply = 0;
    int codec = atoi(argv[4]), solid = atoi(argv[5]);
    check(wimlib_global_init(0));
    if (argv[1][0] == 'w') {
        check(wimlib_create_new_wim(codec, &wim));
        if (codec) check(wimlib_set_output_chunk_size(wim, 32768));
        if (solid) {
            check(wimlib_set_output_pack_compression_type(wim, WIMLIB_COMPRESSION_TYPE_LZMS));
            check(wimlib_set_output_pack_chunk_size(wim, 1048576));
        }
        start = now();
        check(wimlib_add_image(wim, argv[2], "benchmark", NULL, WIMLIB_ADD_FLAG_UNIX_DATA));
        capture = now() - start;
        start = now();
        check(wimlib_write(wim, argv[3], WIMLIB_ALL_IMAGES,
                          WIMLIB_WRITE_FLAG_CHECK_INTEGRITY |
                          (solid ? WIMLIB_WRITE_FLAG_SOLID : 0), 1));
        write = now() - start;
    } else {
        start = now();
        check(wimlib_open_wim(argv[2], 0, &wim));
        open = now() - start;
        start = now();
        check(wimlib_verify_wim(wim, 0));
        verify = now() - start;
        start = now();
        check(wimlib_extract_image(wim, 1, argv[3], WIMLIB_EXTRACT_FLAG_UNIX_DATA));
        apply = now() - start;
    }
    wimlib_free(wim);
    wimlib_global_cleanup();
    struct rusage usage;
    getrusage(RUSAGE_SELF, &usage);
    printf("{\"capture_s\":%.9f,\"write_s\":%.9f,\"open_s\":%.9f,"
           "\"verify_s\":%.9f,\"apply_s\":%.9f,\"peak_rss_kib\":%ld,"
           "\"rusage_peak_rss_kib\":%ld}\n",
           capture, write, open, verify, apply, peak_rss(), usage.ru_maxrss);
    return 0;
}
