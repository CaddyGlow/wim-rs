#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <errno.h>
static void *pointers[256],*outer;static unsigned events;static int failure,reentrant,currentfamily=1,outer_frees,allocated;
static void fb(void *p);
static void *hm(size_t n){events++;void *p=events==(unsigned)failure?NULL:malloc(n);if(events<256)pointers[events]=p;fprintf(stderr,"malloc:%u:%zu:%d\n",events,n,p!=NULL);if(events==1){outer=p;if(p)allocated=1;}if(reentrant)wimlib_set_memory_allocator(hm,fb,NULL);return p;}
static void fa(void *p){if(p&&p==outer){outer_frees++;currentfamily=1;}fprintf(stderr,"free:A:%d:%d\n",p!=NULL,p==outer&&p!=NULL);free(p);}
static void fb(void *p){if(p&&p==outer){outer_frees++;currentfamily=2;}fprintf(stderr,"free:B:%d:%d\n",p!=NULL,p==outer&&p!=NULL);free(p);}
int main(int argc,char **argv){if(argc!=8)return 2;int kind=atoi(argv[1]),codec=atoi(argv[2]);size_t maximum=(size_t)strtoull(argv[3],NULL,10);failure=atoi(argv[4]);int null_output=atoi(argv[5]);int replace=atoi(argv[6]);reentrant=atoi(argv[7]);wimlib_set_memory_allocator(hm,fa,NULL);void *result=(void*)1;int status;errno=0;
 if(kind==0)status=wimlib_create_new_wim(codec,null_output?NULL:(WIMStruct**)&result);
 else if(kind==1)status=wimlib_create_compressor(codec,maximum,50,null_output?NULL:(struct wimlib_compressor**)&result);
 else if(kind==2)status=wimlib_create_decompressor(codec,maximum,null_output?NULL:(struct wimlib_decompressor**)&result);
 else status=wimlib_open_wim(kind==3?"/native-allocator-missing-wim":"docs/wimlib/evidence/native-ffi-write/default-cpu-crash-original.wim",codec,null_output?NULL:(WIMStruct**)&result);
 int saved_errno=errno;if(events)outer=pointers[1];
 printf("result=%d unchanged=%d allocated=%d errno=%d\n",status,result==(void*)1,allocated,saved_errno);
 printf("published-first=%d\n",status==0&&result==outer);
 printf("init-follow=%d\n",wimlib_global_init(-1));
 if(replace)wimlib_set_memory_allocator(hm,fb,NULL);
 if(status==0){if(kind==1)wimlib_free_compressor(result);else if(kind==2)wimlib_free_decompressor(result);else wimlib_free(result);}
 // On failed codec/open initialization the first allocation was freed before
 // outer could be assigned; raw stderr records retain those ownership events.
 if(status!=0&&allocated)printf("error-after-allocation=1\n");
 printf("outer-frees=%d family=%d\n",outer_frees,currentfamily);
 wimlib_free_compressor(NULL);wimlib_free_decompressor(NULL);wimlib_free(NULL);
 wimlib_set_memory_allocator(NULL,NULL,NULL);wimlib_global_cleanup();return 0;}
