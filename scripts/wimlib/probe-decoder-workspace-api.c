#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static unsigned allocations, releases;
static void *hm(size_t size) { void *p=malloc(size);if(p)allocations++;return p; }
static void hf(void *p) { if(p)releases++;free(p); }
static void *hr(void *p,size_t size) { return realloc(p,size); }
int main(int argc,char **argv) {
 (void)argv;
 setvbuf(stdout,NULL,_IONBF,0);
 wimlib_set_memory_allocator(hm,hf,hr);
 for(int codec=1;codec<=3;codec++) {
  struct wimlib_decompressor *d=NULL;int status=wimlib_create_decompressor(codec,65536,&d);printf("create=%d,%d\n",codec,status);if(status)return 1;
  unsigned before=allocations, free_before=releases;
  unsigned char input[258],output[1024];
  for(int pattern=0;pattern<4;pattern++) {
   for(unsigned i=0;i<sizeof(input);i++)input[i]=pattern==0?0:pattern==1?255:pattern==2?(unsigned char)i:(unsigned char)(i*71+13);
   size_t sizes[]={0,1,32,1024};
   for(unsigned s=codec==3&&argc==1?2:0;s<4;s++)for(unsigned repeat=0;repeat<2;repeat++) {
    memset(output,0xcc,sizeof(output));int r=wimlib_decompress(input,sizeof(input),output,sizes[s],d);
    printf("decode=%d,%d,%zu,%u,%d,",codec,pattern,sizes[s],repeat,r);
    if(r==0) {unsigned hash=2166136261u;for(size_t i=0;i<sizes[s];i++)hash=(hash^output[i])*16777619u;printf("%u",hash);}
    puts("");
   }
  }
  printf("reused-no-allocation=%d\n",allocations==before&&releases==free_before);wimlib_free_decompressor(d);
 }
 printf("ownership-balanced=%d\n",allocations==releases);wimlib_set_memory_allocator(NULL,NULL,NULL);wimlib_global_cleanup();return 0;
}
