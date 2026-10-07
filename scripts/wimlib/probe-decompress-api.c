/* Compile unchanged wimlib.h against either library. */
#include <wimlib.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
int main(int argc, char **argv) {
    const size_t sizes[] = {0,1,32768,65536,65537,2097152,2097153,1073741824,1073741825,SIZE_MAX};
    for (int codec=-1; codec<=4; codec++) {
        printf("null %d %d\n", codec, wimlib_create_decompressor(codec,1,NULL));
        for(size_t i=0;i<sizeof(sizes)/sizeof(sizes[0]);i++) {
            struct wimlib_decompressor *dec=(void *)(uintptr_t)1;
            int ret=wimlib_create_decompressor(codec,sizes[i],&dec);
            printf("create %d %zu %d %d\n",codec,sizes[i],ret,ret ? dec==(void *)(uintptr_t)1 : dec!=NULL);
            if(!ret) wimlib_free_decompressor(dec);
        }
    }
    wimlib_free_decompressor(NULL);
    if(argc!=5) return 2;
    int codec=atoi(argv[1]);
    size_t length=strtoull(argv[3],NULL,10), maximum=strtoull(argv[4],NULL,10);
    FILE *file=fopen(argv[2],"rb"); if(!file) return 3;
    fseek(file,0,SEEK_END); size_t packed=ftell(file); rewind(file);
    unsigned char *input=malloc(packed+1), *output=malloc(length+1);
    if(fread(input,1,packed,file)!=packed) return 4; fclose(file);
    struct wimlib_decompressor *dec=NULL;
    if(wimlib_create_decompressor(codec,maximum,&dec)) return 5;
    printf("oversize %d\n",wimlib_decompress(NULL,0,NULL,maximum+1,dec));
    for(int pass=0;pass<3;pass++) {
        memset(output,0xcc,length+1);
        int ret=wimlib_decompress(input,pass==1 ? 0:packed,output,length,dec);
        unsigned long long checksum=14695981039346656037ULL;
        if(!ret) for(size_t i=0;i<length;i++) checksum=(checksum^output[i])*1099511628211ULL;
        printf("decode %d %d %llu %u\n",pass,ret,ret ? 0:checksum,output[length]);
    }
    wimlib_free_decompressor(dec); free(input); free(output); return 0;
}
