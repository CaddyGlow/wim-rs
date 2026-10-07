#include <wimlib.h>
#include <stdint.h>
#include <stdio.h>
int main(int argc, char **argv) {
 WIMStruct *w;
 int codes[]={-1,0,1,2,3,4,100};
 for(unsigned i=0;i<sizeof(codes)/sizeof(codes[0]);i++){
  w=(WIMStruct *)(uintptr_t)1;
  int r=wimlib_create_new_wim(codes[i],&w);
  printf("create %d %d %d\n",codes[i],r,w==(WIMStruct *)(uintptr_t)1);
  if(!r) {
   struct wimlib_wim_info info;
   int result=wimlib_get_wim_info(w,&info);
   unsigned nonzero=0;
   for(unsigned j=0;j<sizeof(info.guid);j++) nonzero|=info.guid[j];
   printf("create-guid %d %d %u\n",codes[i],result,nonzero);
   wimlib_free(w);
  }
  printf("create-null %d %d\n",codes[i],wimlib_create_new_wim(codes[i],NULL));
 }
 wimlib_free(NULL);
 for(int flags=-1;flags<17;flags++) {
  w=(WIMStruct *)(uintptr_t)1;
  int r=wimlib_open_wim(NULL,flags,&w);
  printf("null %d %d %d\n",flags,r,w==(WIMStruct *)(uintptr_t)1);
  r=wimlib_open_wim("",flags,&w);
  printf("empty %d %d %d\n",flags,r,w==(WIMStruct *)(uintptr_t)1);
 }
 for(int i=1;i<argc;i++) for(int flags=0;flags<9;flags++) {
  w=(WIMStruct *)(uintptr_t)1;
  int r=wimlib_open_wim(argv[i],flags,&w);
  printf("open %d %d %d %d\n",i,flags,r,w==(WIMStruct *)(uintptr_t)1);
  if(!r) wimlib_free(w);
 }
 return 0;
}
