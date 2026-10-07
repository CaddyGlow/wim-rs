/* The ABI caller remains the original unchanged-header probe. A duplicate input
 * descriptor drains only after the API has returned, measuring unread bytes
 * without adding reads or seeks to the operation under test. */
#define main pipe_contract_main
#include "probe-pipe-extract.c"
#undef main
#include <unistd.h>
int main(int argc, char **argv)
{
    int observer = dup(0);
    if (observer < 0)
        return 3;
    int result = pipe_contract_main(argc, argv);
    unsigned long long unread = 0;
    char bytes[4096];
    ssize_t count;
    while ((count = read(observer, bytes, sizeof(bytes))) > 0)
        unread += (unsigned long long)count;
    close(observer);
    const char *size = getenv("PIPE_FIXTURE_SIZE");
    if (count < 0 || !size)
        return 4;
    printf("api-read %llu\n", strtoull(size, NULL, 10) - unread);
    return result;
}
