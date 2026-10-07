/* Unchanged-header pipe extraction lifecycle/progress caller. */
#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <fcntl.h>
static int stop;
static enum wimlib_progress_status progress(enum wimlib_progress_msg msg,union wimlib_progress_info *info,void *ctx) {
    (void)ctx;
    const struct wimlib_progress_info_extract *p=&info->extract;
    printf("event %d %u %u %llu %llu %llu %llu %u %u\n",msg,p->image,p->extract_flags,
           (unsigned long long)p->total_bytes,(unsigned long long)p->completed_bytes,
           (unsigned long long)p->total_streams,(unsigned long long)p->completed_streams,
           p->part_number,p->total_parts);
    if(stop>=200 && (int)msg==stop-200)return (enum wimlib_progress_status)2;
    if(stop>=100 && (int)msg==stop-100)return WIMLIB_PROGRESS_STATUS_ABORT;
    return WIMLIB_PROGRESS_STATUS_CONTINUE;
}
int main(int argc,char **argv) {
    if(argc!=5)return 2;
    stop=atoi(argv[4]);errno=123;
    const char *image=!strcmp(argv[1],"NULL")?NULL:argv[1];
    int result=stop==-1 ? wimlib_extract_image_from_pipe(0,image,argv[2],strtol(argv[3],NULL,0))
                        : wimlib_extract_image_from_pipe_with_progress(0,image,argv[2],strtol(argv[3],NULL,0),progress,NULL);
    printf("result %d errno %d\n",result,errno);
    printf("caller-fd %d\n",fcntl(0,F_GETFD)>=0);
    return 0;
}
