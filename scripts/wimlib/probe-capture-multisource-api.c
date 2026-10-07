#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
#include <string.h>
static int status;
static enum wimlib_progress_status progress(enum wimlib_progress_msg msg,union wimlib_progress_info *p,void *ctx)
{
    (void)ctx;
    if(msg>=9&&msg<=11)printf("scan %d %d %llu %llu %llu\n",msg,p->scan.status,(unsigned long long)p->scan.num_dirs_scanned,(unsigned long long)p->scan.num_nondirs_scanned,(unsigned long long)p->scan.num_bytes_scanned);
    else if(msg==21||msg==22)printf("command %d %u %d %zu %zu\n",msg,p->update.command->op,p->update.command->op==0?p->update.command->add.add_flags:0,p->update.completed_commands,p->update.total_commands);
    else printf("progress %d\n",msg);
    if(status==300&&msg==31)p->handle_error.will_ignore=1;
    if(status==400&&msg==30&&strstr(p->test_file_exclusion.path,"/data"))p->test_file_exclusion.will_exclude=1;
    if(status>=200&&status<300&&msg==status-200)return (enum wimlib_progress_status)2;
    if(status>=100&&status<200&&msg==status-100)return WIMLIB_PROGRESS_STATUS_ABORT;
    return WIMLIB_PROGRESS_STATUS_CONTINUE;
}
int main(int argc,char **argv)
{
    if(argc<6||((argc-6)%2))return 2;
    int count=(argc-6)/2,mode=atoi(argv[1]),flags=strtol(argv[2],NULL,0);
    status=atoi(argv[3]);WIMStruct *wim=NULL;int ret=wimlib_create_new_wim(0,&wim);printf("create %d\n",ret);if(ret)return 0;
    wimlib_register_progress_function(wim,progress,NULL);
    struct wimlib_capture_source *sources=calloc(count?count:1,sizeof(*sources));
    struct wimlib_update_command *commands=calloc(count?count:1,sizeof(*commands));
    for(int i=0;i<count;i++){
        sources[i].fs_source_path=argv[6+2*i];sources[i].wim_target_path=strcmp(argv[7+2*i],"NULL")==0?NULL:argv[7+2*i];
        commands[i].op=WIMLIB_UPDATE_OP_ADD;commands[i].add.fs_source_path=sources[i].fs_source_path;commands[i].add.wim_target_path=sources[i].wim_target_path;commands[i].add.add_flags=flags;
    }
    if(mode==3&&count)sources[0].reserved=1;
    if(mode==4)wimlib_add_empty_image(wim,"Capture",NULL);
    errno=123;
    if(mode==1){wimlib_add_empty_image(wim,"Existing",NULL);ret=count?wimlib_add_tree(wim,1,sources[0].fs_source_path,sources[0].wim_target_path,flags):24;}
    else if(mode==2){wimlib_add_empty_image(wim,"Existing",NULL);ret=wimlib_update_image(wim,1,commands,count,1);}
    else ret=wimlib_add_image_multisource(wim,sources,count,"Capture",strcmp(argv[4],"NULL")==0?NULL:argv[4],flags);
    int saved=errno;struct wimlib_wim_info info;wimlib_get_wim_info(wim,&info);printf("add %d images %u boot %u rpfix %u\n",ret,info.image_count,info.boot_index,info.has_rpfix);if(ret)printf("errno %d\n",saved);
    if(!ret){wimlib_register_progress_function(wim,NULL,NULL);ret=wimlib_write(wim,argv[5],WIMLIB_ALL_IMAGES,0,1);printf("write %d\n",ret);if(ret)printf("errno %d\n",errno);}
    free(commands);free(sources);wimlib_free(wim);return 0;
}
