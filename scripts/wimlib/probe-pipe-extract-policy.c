/* Convert explicit test tokens into actual NULL/empty target pointers while
 * retaining the unchanged-header lifecycle caller and post-call IO observer. */
#include <wimlib.h>
#include <string.h>
static const char *policy_target(const char *target)
{
    if (!strcmp(target, "@NULL"))
        return NULL;
    if (!strcmp(target, "@EMPTY"))
        return "";
    return target;
}
static int policy_plain(int fd, const char *image, const char *target, int flags)
{
    return wimlib_extract_image_from_pipe(fd, image, policy_target(target), flags);
}
static int policy_progress(int fd, const char *image, const char *target, int flags,
                           wimlib_progress_func_t progress, void *context)
{
    return wimlib_extract_image_from_pipe_with_progress(fd, image, policy_target(target),
                                                       flags, progress, context);
}
#define wimlib_extract_image_from_pipe policy_plain
#define wimlib_extract_image_from_pipe_with_progress policy_progress
#include "probe-pipe-extract-io.c"
