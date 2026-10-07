#include "wimlib.h"
#include <stdio.h>
#include <stdlib.h>
static enum wimlib_progress_status progress(enum wimlib_progress_msg msg,union wimlib_progress_info *info,void *ctx){
 (void)ctx;
 if(msg==WIMLIB_PROGRESS_MSG_DONE_WITH_FILE)printf("done=%s\n",info->done_with_file.path_to_file);
 if(msg==WIMLIB_PROGRESS_MSG_WRITE_STREAMS)printf("streams=%llu/%llu bytes=%llu/%llu\n",(unsigned long long)info->write_streams.completed_streams,(unsigned long long)info->write_streams.total_streams,(unsigned long long)info->write_streams.completed_bytes,(unsigned long long)info->write_streams.total_bytes);
 return WIMLIB_PROGRESS_STATUS_CONTINUE;
}
static int lookup(const struct wimlib_resource_entry *entry,void *ctx){
 (void)ctx;if(entry->is_metadata)return 0;
 printf("blob=%llu offset=%llu solid=%u hash=",(unsigned long long)entry->uncompressed_size,(unsigned long long)entry->offset,entry->packed);for(int i=0;i<20;i++)printf("%02x",entry->sha1_hash[i]);puts("");return 0;
}
int main(int argc,char **argv){
 if(argc!=6)return 2;
 WIMStruct *w=NULL;int ret=wimlib_create_new_wim(0,&w);int mode=atoi(argv[3]),flags=atoi(argv[4]),codec=atoi(argv[5]);
 if(!ret)ret=wimlib_add_image(w,argv[1],NULL,NULL,WIMLIB_ADD_FLAG_NORPFIX);
 if(!ret&&mode){
  char source[4096];snprintf(source,sizeof(source),"%s.source",argv[2]);
  ret=wimlib_write(w,source,WIMLIB_ALL_IMAGES,mode==2?WIMLIB_WRITE_FLAG_SOLID:0,0);wimlib_free(w);w=NULL;
  if(!ret)ret=wimlib_open_wim(source,0,&w);
 }
 if(!ret)ret=wimlib_set_output_pack_compression_type(w,codec);
 if(!ret)ret=wimlib_set_output_pack_chunk_size(w,32768);
 if(!ret){wimlib_register_progress_function(w,progress,NULL);ret=wimlib_write(w,argv[2],WIMLIB_ALL_IMAGES,flags|WIMLIB_WRITE_FLAG_SEND_DONE_WITH_FILE_MESSAGES,0);}
 printf("write=%d\n",ret);wimlib_free(w);w=NULL;
 if(!ret)ret=wimlib_open_wim(argv[2],0,&w);
 if(!ret)ret=wimlib_iterate_lookup_table(w,0,lookup,NULL);
 printf("read=%d\n",ret);wimlib_free(w);return 0;
}
