#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
#include <string.h>
static int calls,fail;static void *tracked;
static void *hm(size_t n){calls++;void *p=calls==fail?NULL:malloc(n);printf("malloc=%zu,%d\n",n,p!=NULL);tracked=p;return p;}
static void hf(void *p){printf("free=%d,%d\n",p!=NULL,p==tracked);free(p);}
static void *hr(void *p,size_t n){calls++;void *q=calls==fail?NULL:realloc(p,n);printf("realloc=%zu,%d\n",n,q!=NULL);return q;}
int main(int argc,char **argv){if(argc!=4)return 2;WIMStruct *w=NULL;int r=!strcmp(argv[1],"@EMPTY")?wimlib_create_new_wim(0,&w):wimlib_open_wim(argv[1],0,&w);printf("open=%d\n",r);if(r)return 0;fail=atoi(argv[2]);int extract=atoi(argv[3]);wimlib_set_memory_allocator(hm,hf,hr);errno=0;
 if(!extract){void *p=(void*)1;size_t n=777;r=wimlib_get_xml_data(w,&p,&n);printf("get=%d,%d,%zu,%d\n",r,p==(void*)1,n,errno);if(!r){unsigned char *b=p;printf("bytes=");for(size_t i=0;i<n;i++)printf("%02x",b[i]);puts("");hf(p);}}
 else {FILE *f=tmpfile();r=wimlib_extract_xml_data(w,f);printf("extract=%d,%d\n",r,errno);fclose(f);}
 wimlib_set_memory_allocator(NULL,NULL,NULL);wimlib_free(w);return 0;}
