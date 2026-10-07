/* Original unchanged-header caller: real split pipable extraction and read volume. */
#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
#include <fcntl.h>
#include <unistd.h>
static int stop_part, status, seen_parts;
static enum wimlib_progress_status progress(enum wimlib_progress_msg msg,
        union wimlib_progress_info *info, void *context)
{
    (void)context;
    const struct wimlib_progress_info_extract *p = &info->extract;
    printf("event %d %u %u %llu %llu %llu %llu %u %u", msg, p->image,
        p->extract_flags, (unsigned long long)p->total_bytes,
        (unsigned long long)p->completed_bytes, (unsigned long long)p->total_streams,
        (unsigned long long)p->completed_streams, p->part_number, p->total_parts);
    if (msg == WIMLIB_PROGRESS_MSG_EXTRACT_SPWM_PART_BEGIN) {
        ++seen_parts;
        printf(" guid ");
        for (unsigned i = 0; i < 16; ++i) printf("%02x", p->guid[i]);
    }
    putchar('\n');
    if (msg == WIMLIB_PROGRESS_MSG_EXTRACT_SPWM_PART_BEGIN && seen_parts == stop_part)
        return status ? (enum wimlib_progress_status)2 : WIMLIB_PROGRESS_STATUS_ABORT;
    return WIMLIB_PROGRESS_STATUS_CONTINUE;
}
int main(int argc, char **argv)
{
    if (argc != 5) return 2;
    stop_part = atoi(argv[2]); status = atoi(argv[3]);
    int observer = dup(0);
    if (observer < 0) return 3;
    errno = 123;
    int result = wimlib_extract_image_from_pipe_with_progress(0, "1", argv[1],
        strtol(argv[4], NULL, 0), progress, NULL);
    printf("result %d errno %d\ncaller-fd %d\n", result, errno, fcntl(0, F_GETFD) >= 0);
    unsigned long long unread = 0;
    char bytes[4096]; ssize_t count;
    while ((count = read(observer, bytes, sizeof(bytes))) > 0) unread += (unsigned long long)count;
    close(observer);
    const char *size = getenv("PIPE_FIXTURE_SIZE");
    if (count < 0 || !size) return 4;
    printf("api-read %llu\n", strtoull(size, NULL, 10) - unread);
    return 0;
}
