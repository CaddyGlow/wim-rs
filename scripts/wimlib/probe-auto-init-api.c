#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
#include <string.h>
static enum wimlib_progress_status progress(enum wimlib_progress_msg m,union wimlib_progress_info *i,void *c) {(void)m;(void)i;(void)c;return WIMLIB_PROGRESS_STATUS_CONTINUE;}
int main(int argc,char **argv) {
 if(argc!=4)return 2;
 int kind=atoi(argv[1]),variant=atoi(argv[2]),follow=atoi(argv[3]);
 WIMStruct *w=(WIMStruct*)1;int r;
 wimlib_set_print_errors(true);
 const char *path=variant==0?NULL:variant==1?"":"/native-auto-init-no-such-file";
 errno=0;
 if(kind==0)r=wimlib_create_new_wim(variant==2?-1:0,variant==0?NULL:&w);
 else if(kind==1)r=wimlib_open_wim(path,variant==3?8:0,variant==4?NULL:&w);
 else r=wimlib_open_wim_with_progress(path,variant==3?8:0,variant==4?NULL:&w,progress,NULL);
 printf("call=%d unchanged=%d\n",r,w==(WIMStruct*)1);
 if(r==0 && w!=(WIMStruct*)1)wimlib_free(w);
 if(kind!=0 && variant!=2)wimlib_set_print_errors(false);
 char *text=(char*)1;size_t n=7;
 printf("text=%d\n",wimlib_load_text_file("/native-auto-init-no-such-text",&text,&n));
 printf("follow=%d\n",wimlib_global_init(follow));
 wimlib_global_cleanup();
 printf("after-cleanup=%d\n",wimlib_global_init(follow));
 wimlib_global_cleanup();return 0;
}
