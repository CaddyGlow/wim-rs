/* Unchanged-header text loader client; successful output is released by C free. */
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wimlib.h>
int main(int argc,char **argv){
    if(argc<2||argc>3)return 2;
    if(argc==3 && !strcmp(argv[2],"pushback")) { if(ungetc('X',stdin)==EOF)return 3; }
    if(argc==3 && !strcmp(argv[2],"closed"))close(0);
    const char *path=!strcmp(argv[1],"@NULL")?NULL:argv[1];
    wimlib_tchar *output=(wimlib_tchar *)(uintptr_t)0x1234;
    size_t length=123456;
    errno=E2BIG;
    int result=wimlib_load_text_file(path,&output,&length);
    int saved_errno=errno;
    printf("result=%d\nerrno=%d\n",result,saved_errno);
    if(result){printf("pointer_unchanged=%d length=%zu\n",output==(wimlib_tchar *)(uintptr_t)0x1234,length);return 0;}
    printf("length=%zu terminator=%d\n",length,output[length]==0);
    for(size_t i=0;i<length;i++)printf("%02x",(unsigned char)output[i]);putchar('\n');
    free(output);return 0;
}
