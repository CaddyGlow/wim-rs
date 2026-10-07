#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
static int status;
static enum wimlib_progress_status progress(enum wimlib_progress_msg message, union wimlib_progress_info *info, void *context)
{
    (void)context;
    struct wimlib_progress_info_extract *p = &info->extract;
    printf("progress %d %u %u %llu %llu %llu %llu %llu %llu\n", message, p->image, p->extract_flags,
           (unsigned long long)p->total_bytes, (unsigned long long)p->completed_bytes,
           (unsigned long long)p->total_streams, (unsigned long long)p->completed_streams,
           (unsigned long long)p->current_file_count, (unsigned long long)p->end_file_count);
    fflush(stdout);
    if (status >= 200) return message == status - 200 ? (enum wimlib_progress_status)2 : WIMLIB_PROGRESS_STATUS_CONTINUE;
    if (status >= 100) return message == status - 100 ? WIMLIB_PROGRESS_STATUS_ABORT : WIMLIB_PROGRESS_STATUS_CONTINUE;
    return (enum wimlib_progress_status)status;
}
int main(int argc, char **argv)
{
    if (argc < 7) return 2;
    WIMStruct *wim = NULL;
    int raw_mode = atoi(argv[2]);
    int mode = raw_mode & 255;
    if (raw_mode & 256) printf("init %d\n", wimlib_global_init(WIMLIB_INIT_FLAG_DEFAULT_CASE_INSENSITIVE));
    int ret = mode >= 2 ? wimlib_create_new_wim(0, &wim) : wimlib_open_wim(argv[1], 0, &wim);
    printf("open %d\n", ret);
    fflush(stdout);
    if (ret) return 0;
    if (mode >= 2) {
        ret = wimlib_add_empty_image(wim, "Pending", NULL);
        printf("add-empty %d\n",ret);
        if (ret) { wimlib_free(wim); return 0; }
    }
    status = atoi(argv[6]);
    wimlib_register_progress_function(wim, progress, NULL);
    errno = 123;
    const char *target = raw_mode & 2048 ? NULL : raw_mode & 4096 ? "" : argv[4];
    WIMStruct *call_wim = raw_mode & 8192 ? NULL : wim;
    const char *null_path = NULL;
    const char * const *paths = (const char * const *)(argv + 7);
    size_t num_paths = argc - 7;
    if (raw_mode & 512) { paths = NULL; num_paths = 1; }
    if (raw_mode & 1024) { paths = &null_path; num_paths = 1; }
    if (mode == 0 || mode == 2)
        ret = wimlib_extract_paths(call_wim, atoi(argv[3]), target, paths, num_paths, strtol(argv[5], NULL, 0));
    else if (mode == 3)
        ret = wimlib_extract_image(call_wim, atoi(argv[3]), target, strtol(argv[5], NULL, 0));
    else
        ret = wimlib_extract_pathlist(call_wim, atoi(argv[3]), target, argc == 7 ? NULL : argv[7], strtol(argv[5], NULL, 0));
    int saved_errno = errno;
    printf("extract %d\n",ret);
    if (ret) printf("errno %d\n",saved_errno);
    wimlib_free(wim);
    return 0;
}
