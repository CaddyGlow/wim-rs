#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
static int status;
static enum wimlib_progress_status progress(enum wimlib_progress_msg message, union wimlib_progress_info *info, void *context)
{
    (void)context;
    struct wimlib_progress_info_extract *p = &info->extract;
    printf("progress %d %u %u %llu %llu %llu %llu %llu %llu\n", message, p->image, p->extract_flags,
           (unsigned long long)p->total_bytes, (unsigned long long)p->completed_bytes,
           (unsigned long long)p->total_streams, (unsigned long long)p->completed_streams,
           (unsigned long long)p->current_file_count, (unsigned long long)p->end_file_count);
    if (status >= 200) return message == status - 200 ? (enum wimlib_progress_status)2 : WIMLIB_PROGRESS_STATUS_CONTINUE;
    if (status >= 100) return message == status - 100 ? WIMLIB_PROGRESS_STATUS_ABORT : WIMLIB_PROGRESS_STATUS_CONTINUE;
    return (enum wimlib_progress_status)status;
}
int main(int argc, char **argv)
{
    if (argc < 6) return 2;
    WIMStruct *wim = NULL;
    int ret = wimlib_open_wim(argv[1], 0, &wim);
    printf("open %d\n", ret);
    if (ret) return 0;
    if (argc > 6) printf("reference %d\n", wimlib_reference_resource_files(wim, (const char * const *)(argv + 6), argc - 6, 0, 0));
    status = atoi(argv[5]);
    wimlib_register_progress_function(wim, progress, NULL);
    printf("extract %d\n", wimlib_extract_image(wim, atoi(argv[2]), argv[3], strtol(argv[4], NULL, 0)));
    wimlib_free(wim);
    return 0;
}
