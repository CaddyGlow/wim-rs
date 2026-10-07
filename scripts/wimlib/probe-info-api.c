/* Test-only C client compiled against the unchanged original public header. */
#include <wimlib.h>
#include <stdio.h>
#include <limits.h>
int main(void) {
    printf("version:%u:%s\n", wimlib_get_version(), wimlib_get_version_string());
    for (int code = -2; code <= 202; code++)
        printf("error:%d:%s\n", code, wimlib_get_error_string(code));
    printf("error:%d:%s\n", INT_MIN, wimlib_get_error_string(INT_MIN));
    printf("error:%d:%s\n", INT_MAX, wimlib_get_error_string(INT_MAX));
    for (int code = -2; code <= 5; code++)
        printf("compression:%d:%s\n", code, wimlib_get_compression_type_string(code));
    return 0;
}
