/* Caller compiled unchanged against original test_support.h. */
#include "config.h"
#include "wimlib.h"
#include "wimlib/test_support.h"
#include <stdio.h>
#include <stdlib.h>
int main(int argc, char **argv) {
    WIMStruct *a = NULL, *b = NULL;
    if (argc != 6) return 2;
    int ra = wimlib_open_wim(argv[1],0,&a);
    int rb = wimlib_open_wim(argv[3],0,&b);
    printf("open=%d,%d\n",ra,rb);
    if (!ra && !rb) printf("compare=%d\n",wimlib_compare_images(a,atoi(argv[2]),b,atoi(argv[4]),atoi(argv[5])));
    wimlib_free(a); wimlib_free(b);
    return 0;
}
