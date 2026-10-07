#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static const char *retry,*source;
static int lookup(const struct wimlib_resource_entry *entry, void *ctx) {
 (void)ctx;
 if(entry->is_metadata)return 0;
 printf("blob=%llu refs=%u hash=",(unsigned long long)entry->uncompressed_size,entry->reference_count);
 for(unsigned i=0;i<20;i++)printf("%02x",entry->sha1_hash[i]);
 puts("");return 0;
}
static void finish(WIMStruct *w) {
 wimlib_register_progress_function(w,NULL,NULL);
 printf("lookup=%d\n",wimlib_iterate_lookup_table(w,0,lookup,NULL));
 printf("verify=%d\n",wimlib_verify_wim(w,0));
 if(getenv("MODIFY_AFTER_DONE")) {
  char *path=malloc(strlen(source)+3);sprintf(path,"%s/a",source);
  FILE *file=fopen(path,"r+b");if(file){fputc('1',file);fclose(file);}free(path);
  printf("verify_modified=%d\n",wimlib_verify_wim(w,0));
 }
 printf("retry=%d\n",wimlib_write(w,retry,WIMLIB_ALL_IMAGES,0x2000,1));
 wimlib_free(w);
}
#define wimlib_free finish
#define main original_main
#include "probe-done-with-file.c"
#undef main
#undef wimlib_free
int main(int argc,char **argv) {
 if(argc!=7)return 2;
 source=argv[1];
 char *path=malloc(strlen(argv[2])+7);sprintf(path,"%s.retry",argv[2]);retry=path;
 int result=original_main(argc,argv);free(path);return result;
}
