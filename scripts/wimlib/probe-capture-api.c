#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
#include <unistd.h>
#include <fcntl.h>
#include <string.h>
static int status;
static int resource(const struct wimlib_resource_entry *entry,void *ctx)
{
    (void)ctx;
    printf("resource %llu %u %u %u ",(unsigned long long)entry->uncompressed_size,entry->reference_count,entry->is_metadata,entry->is_missing);
    for(int i=0;i<20;i++)printf("%02x",entry->sha1_hash[i]);
    putchar('\n');return 0;
}
static enum wimlib_progress_status progress(enum wimlib_progress_msg msg, union wimlib_progress_info *info, void *ctx)
{
    (void)ctx;
    if (msg >= 9 && msg <= 11) {
        printf("scan %d %d %llu %llu %llu\n", msg, info->scan.status,
               (unsigned long long)info->scan.num_dirs_scanned,
               (unsigned long long)info->scan.num_nondirs_scanned,
               (unsigned long long)info->scan.num_bytes_scanned);
    } else if (msg >= 0 && msg <= 8) {
        printf("extract-progress %d %llu %llu %llu %llu %llu %llu\n",msg,
               (unsigned long long)info->extract.total_bytes,(unsigned long long)info->extract.completed_bytes,
               (unsigned long long)info->extract.total_streams,(unsigned long long)info->extract.completed_streams,
               (unsigned long long)info->extract.current_file_count,(unsigned long long)info->extract.end_file_count);
    } else if (msg == WIMLIB_PROGRESS_MSG_WRITE_STREAMS) {
        printf("write-progress %llu %llu %llu %llu\n",
               (unsigned long long)info->write_streams.total_bytes,
               (unsigned long long)info->write_streams.total_streams,
               (unsigned long long)info->write_streams.completed_bytes,
               (unsigned long long)info->write_streams.completed_streams);
    } else printf("progress %d\n",msg);
    if (status >= 200 && msg == status-200) return (enum wimlib_progress_status)2;
    if (status >= 100 && msg == status-100) return WIMLIB_PROGRESS_STATUS_ABORT;
    return WIMLIB_PROGRESS_STATUS_CONTINUE;
}
int main(int argc,char **argv)
{
    if(argc<7) return 2;
    WIMStruct *wim=NULL;
    const char *compression=getenv("CAPTURE_COMPRESSION");
    int ret=wimlib_create_new_wim(compression?atoi(compression):0,&wim);
    printf("create %d\n",ret); if(ret) return 0;
    status=atoi(argv[5]); wimlib_register_progress_function(wim,progress,NULL);
    errno=123;
    ret=wimlib_add_image(wim,argv[1],"Capture",strcmp(argv[3],"NULL")==0?NULL:argv[3],strtol(argv[2],NULL,0));
    int saved=errno;
    struct wimlib_wim_info info;
    wimlib_get_wim_info(wim,&info);
    printf("add %d images %u boot %u rpfix %u\n",ret,info.image_count,info.boot_index,info.has_rpfix);
    if(ret) printf("errno %d\n",saved);
    if(!ret) {
        if(argc>8) { printf("lookup-before\n");printf("lookup-status %d\n",wimlib_iterate_lookup_table(wim,0,resource,NULL)); }
        int mutation=atoi(argv[6]);
        if(argc>7 && mutation) {
            if(mutation==1) unlink(argv[7]);
            else { int fd=open(argv[7],O_WRONLY|O_TRUNC); if(fd>=0) {
                if(mutation==2) write(fd,"later",5);
                if(mutation==4) write(fd,"later data!!",12);
                if(mutation==5) write(fd,"later data!! and extra",22);
                close(fd); } }
        }
        const char *extract_target=getenv("CAPTURE_EXTRACT_TARGET");
        if(extract_target){
            const char *extract_flags=getenv("CAPTURE_EXTRACT_FLAGS");
            errno=123;int extract=wimlib_extract_image(wim,1,extract_target,extract_flags?strtol(extract_flags,NULL,0):0);
            int extract_errno=errno;printf("extract %d\n",extract);if(extract)printf("extract-errno %d\n",extract_errno);
        }
        errno=123;
        const char *write_flags=getenv("CAPTURE_WRITE_FLAGS");
        ret=wimlib_write(wim,argv[4],WIMLIB_ALL_IMAGES,write_flags?strtol(write_flags,NULL,0):0,1);
        saved=errno; printf("write %d\n",ret); if(ret) printf("errno %d\n",saved);
        if(argc>8) { printf("lookup-after\n");printf("lookup-status %d\n",wimlib_iterate_lookup_table(wim,0,resource,NULL)); }
    }
    wimlib_free(wim);return 0;
}
