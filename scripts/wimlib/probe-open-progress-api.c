#include <wimlib.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>

struct context { int count, stop, status; };
static enum wimlib_progress_status
progress(enum wimlib_progress_msg message, union wimlib_progress_info *info,
         void *opaque)
{
    struct context *ctx = opaque;
    ctx->count++;
    if (message == WIMLIB_PROGRESS_MSG_VERIFY_INTEGRITY) {
        printf("integrity:%" PRIu64 ":%" PRIu64 ":%u:%u:%u:%s\n",
               info->integrity.total_bytes, info->integrity.completed_bytes,
               info->integrity.total_chunks, info->integrity.completed_chunks,
               info->integrity.chunk_size, info->integrity.filename);
    } else {
        printf("event:%d\n", message);
    }
    return ctx->count == ctx->stop ? ctx->status : WIMLIB_PROGRESS_STATUS_CONTINUE;
}
int main(int argc, char **argv)
{
    if (argc != 5) return 2;
    struct context ctx = {0, atoi(argv[3]), atoi(argv[4])};
    WIMStruct *wim = (WIMStruct *)(uintptr_t)0x1234;
    int null_name = wimlib_open_wim_with_progress(NULL, 1, &wim, progress, &ctx);
    int empty_name = wimlib_open_wim_with_progress("", 1, &wim, progress, &ctx);
    int null_output = wimlib_open_wim_with_progress(argv[1], 1, NULL, progress, &ctx);
    printf("invalid:%d:%d:%d:%d:%d\n", null_name, empty_name, null_output,
           ctx.count, wim == (WIMStruct *)(uintptr_t)0x1234);
    int result = wimlib_open_wim_with_progress(argv[1], atoi(argv[2]), &wim,
                                              progress, &ctx);
    printf("open:%d:%d:%d\n", result, ctx.count,
           result ? wim == (WIMStruct *)(uintptr_t)0x1234 : wim != NULL);
    if (!result) wimlib_free(wim);
    return 0;
}
