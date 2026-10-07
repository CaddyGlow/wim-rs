#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
static enum wimlib_progress_status progress(enum wimlib_progress_msg msg,union wimlib_progress_info *info,void *ctx) {
    (void)ctx;
    if(msg==27 || msg==28) printf("image %d %u %u\n",msg,info->verify_image.current_image,info->verify_image.total_images);
    if(msg==29) printf("streams %llu %llu %llu %llu\n",(unsigned long long)info->verify_streams.total_streams,(unsigned long long)info->verify_streams.total_bytes,(unsigned long long)info->verify_streams.completed_streams,(unsigned long long)info->verify_streams.completed_bytes);
    return WIMLIB_PROGRESS_STATUS_CONTINUE;
}
int main(int argc,char **argv) {
    if(argc!=4)return 2;
    WIMStruct *w=NULL;int status=wimlib_create_new_wim(0,&w);printf("create %d\n",status);if(status)return 0;
    wimlib_register_progress_function(w,progress,NULL);
    status=wimlib_add_image(w,argv[1],"Verify",NULL,0);printf("add %d\n",status);
    if(!status){
        printf("before %d\n",wimlib_verify_wim(w,0));
        status=wimlib_write(w,argv[2],WIMLIB_ALL_IMAGES,0,1);printf("write %d\n",status);
        if(!status){
            char path[4096];snprintf(path,sizeof(path),"%s/file",argv[1]);
            int mode=atoi(argv[3]);
            if(mode==3)unlink(path);
            else if(mode){FILE *f=fopen(path,"r+b");if(f){if(mode==1)fputc('!',f);else ftruncate(fileno(f),1);fclose(f);}}
            printf("after %d\n",wimlib_verify_wim(w,0));
        }
    }
    wimlib_free(w);return 0;
}
