#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
#include <string.h>
static int count,fail_at,zero_fails,reenter;static void *tracked[4096];static unsigned nextid;
static unsigned id(void *p) {if(!p)return 0;for(unsigned i=1;i<=nextid;i++)if(tracked[i]==p)return i;return 9999;}
static unsigned record(void *p) {if(!p)return 0;tracked[++nextid]=p;return nextid;}
static void *hm(size_t n) {count++;printf("m:%zu",n);void *p=(count==fail_at || (!n&&zero_fails))?NULL:malloc(n);printf("=%u\n",record(p));if(reenter)wimlib_set_memory_allocator(hm,NULL,NULL);return p;}
static void hf(void *p) {printf("f:%u\n",id(p));if(p){for(unsigned i=1;i<=nextid;i++)if(tracked[i]==p)tracked[i]=NULL;}free(p);}
static void *hr(void *p,size_t n) {unsigned old=id(p);count++;void *q=count==fail_at?NULL:realloc(p,n);printf("r:%u:%zu",old,n);if(q&&p){for(unsigned i=1;i<=nextid;i++)if(tracked[i]==p)tracked[i]=NULL;}printf("=%u\n",record(q));return q;}
int main(int argc,char **argv) {
 if(argc!=6)return 2;fail_at=atoi(argv[2]);zero_fails=atoi(argv[3]);int mask=atoi(argv[4]);reenter=atoi(argv[5]);
 printf("set=%d\n",wimlib_set_memory_allocator(mask&1?hm:NULL,mask&2?hf:NULL,mask&4?hr:NULL));
 char *text=(char*)1;size_t length=777;errno=0;
 int r=wimlib_load_text_file(!strcmp(argv[1],"@STDIN")?NULL:argv[1],&text,&length);int saved=errno;
 printf("result=%d errno=%d untouched=%d length=%zu\n",r,saved,text==(char*)1,length);
 if(r==0) {printf("text=");for(size_t i=0;i<=length;i++)printf("%02x",(unsigned char)text[i]);puts("");if(mask&2)hf(text);else free(text);}
 printf("reset=%d\n",wimlib_set_memory_allocator(NULL,NULL,NULL));return 0;
}
