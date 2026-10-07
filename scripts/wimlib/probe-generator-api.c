#include "config.h"
#include "wimlib.h"
#include "wimlib/test_support.h"
#include <stdio.h>
#include <stdlib.h>
static int stop=-1;
static enum wimlib_progress_status progress(enum wimlib_progress_msg msg, union wimlib_progress_info *info, void *ctx){
    (void)ctx;
    if(msg==WIMLIB_PROGRESS_MSG_SCAN_BEGIN||msg==WIMLIB_PROGRESS_MSG_SCAN_END)
        printf("event=%d dirs=%llu files=%llu bytes=%llu current=%d\n",msg,(unsigned long long)info->scan.num_dirs_scanned,(unsigned long long)info->scan.num_nondirs_scanned,(unsigned long long)info->scan.num_bytes_scanned,info->scan.cur_path!=NULL);
    return msg==stop?WIMLIB_PROGRESS_STATUS_ABORT:WIMLIB_PROGRESS_STATUS_CONTINUE;
}
static int lookup(const struct wimlib_resource_entry *entry,void *ctx){
    (void)ctx;
    printf("blob=%llu refs=%u flags=%d%d offset=%llu packed=%llu part=%u hash=",(unsigned long long)entry->uncompressed_size,entry->reference_count,entry->is_metadata,entry->is_compressed,(unsigned long long)entry->offset,(unsigned long long)entry->compressed_size,entry->part_number);
    for(int i=0;i<20;i++)printf("%02x",entry->sha1_hash[i]);puts("");return 0;
}
int main(int argc,char **argv){
    if(argc!=5&&argc!=6)return 2;
    WIMStruct *w=NULL;int ret=wimlib_create_new_wim(0,&w);stop=atoi(argv[3]);
    wimlib_seed_random(strtoull(argv[1],NULL,0));wimlib_register_progress_function(w,progress,NULL);
    if(!ret)ret=wimlib_add_image(w,(void*)1,NULL,argc==6?argv[5]:NULL,WIMLIB_ADD_FLAG_GENERATE_TEST_DATA|WIMLIB_ADD_FLAG_NORPFIX);
    struct wimlib_wim_info wi;wimlib_get_wim_info(w,&wi);
    printf("add=%d count=%u\n",ret,wi.image_count);
    printf("lookup=%d\n",wimlib_iterate_lookup_table(w,0,lookup,NULL));
    wimlib_register_progress_function(w,NULL,NULL);
    if(!ret)printf("write=%d\n",wimlib_write(w,argv[2],WIMLIB_ALL_IMAGES,atoi(argv[4]),0));
    wimlib_free(w);return 0;
}
