#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <unistd.h>
#include <sys/stat.h>
static int stop,remove_source;
static enum wimlib_progress_status callback(enum wimlib_progress_msg msg, union wimlib_progress_info *info, void *ctx) {

 printf("event=%d",msg);
 if(msg==WIMLIB_PROGRESS_MSG_DONE_WITH_FILE) {
  printf(" path=%s",info->done_with_file.path_to_file);
  if(remove_source==1)printf(" unlink=%d",unlink(info->done_with_file.path_to_file));
  if(remove_source==2)wimlib_register_progress_function(ctx,NULL,NULL);
  if(remove_source==4) { puts(""); return (enum wimlib_progress_status)2; }
 }
 if(msg==WIMLIB_PROGRESS_MSG_WRITE_STREAMS)printf(" bytes=%llu/%llu streams=%llu/%llu",(unsigned long long)info->write_streams.completed_bytes,(unsigned long long)info->write_streams.total_bytes,(unsigned long long)info->write_streams.completed_streams,(unsigned long long)info->write_streams.total_streams);
 puts("");
 return (int)msg==stop ? WIMLIB_PROGRESS_STATUS_ABORT : WIMLIB_PROGRESS_STATUS_CONTINUE;
}
int main(int argc,char **argv) {
 if(argc!=7)return 2;
 WIMStruct *w=NULL;
 stop=atoi(argv[5]);remove_source=atoi(argv[6]);
 int ret=wimlib_create_new_wim(atoi(argv[3]),&w);
 if(!ret && strcmp(argv[1],"@NEW"))ret=wimlib_add_image(w,argv[1],NULL,NULL,0);
 printf("capture=%d\n",ret);
 if(!ret){wimlib_register_progress_function(w,callback,w);errno=0;ret=wimlib_write(w,argv[2],WIMLIB_ALL_IMAGES,atoi(argv[4]),1);}
 int saved=errno;struct stat st;int exists=stat(argv[2],&st)==0;
 printf("result=%d errno=%d exists=%d size=%llu\n",ret,saved,exists,exists?(unsigned long long)st.st_size:0);
 wimlib_free(w);return 0;
}
