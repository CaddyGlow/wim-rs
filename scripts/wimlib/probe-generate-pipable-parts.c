/* Generate real split parts through the unchanged original C API. */
#include <wimlib.h>
#include <stdlib.h>
int main(int argc, char **argv)
{
    if (argc != 4) return 2;
    WIMStruct *wim = NULL;
    int result = wimlib_open_wim(argv[1], 0, &wim);
    if (!result) result = wimlib_split(wim, argv[2], strtoull(argv[3], NULL, 10), WIMLIB_WRITE_FLAG_PIPABLE);
    wimlib_free(wim);
    return result;
}
