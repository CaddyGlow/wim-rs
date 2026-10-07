/* SPDX-License-Identifier: LGPL-2.1-or-later */
#include <wimlib.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
struct context {WIMStruct *wim;int id,status,stop,action,changed;struct context *replacement;};
static enum wimlib_progress_status callback(enum wimlib_progress_msg msg,union wimlib_progress_info *info,void *opaque){
 struct context *ctx=opaque;
 printf("event %d %d",ctx->id,msg);
 if(msg==WIMLIB_PROGRESS_MSG_WRITE_METADATA_BEGIN || msg==WIMLIB_PROGRESS_MSG_WRITE_METADATA_END)printf(" info-null %d",info==NULL);
 if(msg==WIMLIB_PROGRESS_MSG_BEGIN_VERIFY_IMAGE || msg==WIMLIB_PROGRESS_MSG_END_VERIFY_IMAGE)printf(" image %u %u file %s",info->verify_image.current_image,info->verify_image.total_images,info->verify_image.wimfile?info->verify_image.wimfile:"NULL");
 if(msg==WIMLIB_PROGRESS_MSG_VERIFY_STREAMS)printf(" streams %llu %llu bytes %llu %llu file %s",(unsigned long long)info->verify_streams.completed_streams,(unsigned long long)info->verify_streams.total_streams,(unsigned long long)info->verify_streams.completed_bytes,(unsigned long long)info->verify_streams.total_bytes,info->verify_streams.wimfile?info->verify_streams.wimfile:"NULL");
 if(msg==WIMLIB_PROGRESS_MSG_SPLIT_BEGIN_PART || msg==WIMLIB_PROGRESS_MSG_SPLIT_END_PART)printf(" split %u %u bytes %llu %llu name %s",info->split.cur_part_number,info->split.total_parts,(unsigned long long)info->split.completed_bytes,(unsigned long long)info->split.total_bytes,info->split.part_name);
 if(msg==WIMLIB_PROGRESS_MSG_WRITE_STREAMS)printf(" write streams %llu %llu bytes %llu %llu threads %u codec %d parts %u compressed %llu",(unsigned long long)info->write_streams.completed_streams,(unsigned long long)info->write_streams.total_streams,(unsigned long long)info->write_streams.completed_bytes,(unsigned long long)info->write_streams.total_bytes,info->write_streams.num_threads,info->write_streams.compression_type,info->write_streams.total_parts,(unsigned long long)info->write_streams.completed_compressed_bytes);
 if(msg==WIMLIB_PROGRESS_MSG_VERIFY_INTEGRITY || msg==WIMLIB_PROGRESS_MSG_CALC_INTEGRITY)printf(" integrity %u %u bytes %llu %llu chunk %u",info->integrity.completed_chunks,info->integrity.total_chunks,(unsigned long long)info->integrity.completed_bytes,(unsigned long long)info->integrity.total_bytes,info->integrity.chunk_size);
 puts("");
 if(ctx->action==4 && msg==WIMLIB_PROGRESS_MSG_SPLIT_BEGIN_PART && !ctx->changed){ctx->changed=1;info->split.part_name="/tmp/wim-progress-replaced.swm";}
 if(!ctx->changed && ((ctx->action==1 || ctx->action==2) || (ctx->action==3 && msg==WIMLIB_PROGRESS_MSG_VERIFY_STREAMS) || ((ctx->action==6 || ctx->action==7) && msg==WIMLIB_PROGRESS_MSG_WRITE_METADATA_BEGIN))){ctx->changed=1;wimlib_register_progress_function(ctx->wim,(ctx->action==2 || ctx->action==7)?callback:NULL,(ctx->action==2 || ctx->action==7)?ctx->replacement:NULL);}
 if(ctx->action==5 && msg==WIMLIB_PROGRESS_MSG_WRITE_STREAMS && info->write_streams.completed_bytes==0)return WIMLIB_PROGRESS_STATUS_CONTINUE;
 return ctx->stop==0 || ctx->stop==(int)msg ? (enum wimlib_progress_status)ctx->status:WIMLIB_PROGRESS_STATUS_CONTINUE;
}
static int state_resource(const struct wimlib_resource_entry *entry,void *unused){(void)unused;if(entry->is_metadata){unsigned zero=1;for(unsigned i=0;i<20;i++)if(entry->sha1_hash[i])zero=0;printf("metadata-state %llu hash-zero %u\n",(unsigned long long)entry->uncompressed_size,zero);}return 0;}
int main(int argc,char **argv){
 printf("layout union %zu align %zu image %zu streams %zu write %zu integrity %zu split %zu\n",sizeof(union wimlib_progress_info),_Alignof(union wimlib_progress_info),sizeof(struct wimlib_progress_info_verify_image),sizeof(struct wimlib_progress_info_verify_streams),sizeof(struct wimlib_progress_info_write_streams),sizeof(struct wimlib_progress_info_integrity),sizeof(struct wimlib_progress_info_split));
 printf("offset image %zu %zu %zu streams %zu %zu %zu %zu %zu\n",offsetof(struct wimlib_progress_info_verify_image,wimfile),offsetof(struct wimlib_progress_info_verify_image,total_images),offsetof(struct wimlib_progress_info_verify_image,current_image),offsetof(struct wimlib_progress_info_verify_streams,wimfile),offsetof(struct wimlib_progress_info_verify_streams,total_streams),offsetof(struct wimlib_progress_info_verify_streams,total_bytes),offsetof(struct wimlib_progress_info_verify_streams,completed_streams),offsetof(struct wimlib_progress_info_verify_streams,completed_bytes));
 if(argc!=6 && argc!=7)return 2;
 WIMStruct *wim=NULL;int rc=(!strcmp(argv[1],"new")||!strcmp(argv[1],"empty"))?wimlib_create_new_wim(1,&wim):wimlib_open_wim(argv[1],0,&wim);if(rc){printf("open %d\n",rc);return 0;}
 if(!strcmp(argv[1],"empty")){rc=wimlib_add_empty_image(wim,"empty",NULL);if(rc){printf("add %d\n",rc);wimlib_free(wim);return 0;}}
 if(getenv("WIM_PROGRESS_INVALID_XML")){rc=wimlib_set_image_property(wim,1,"DESCRIPTION","\xff");printf("invalid-xml-set %d\n",rc);}
 if(getenv("WIM_PROGRESS_CODEC")){rc=wimlib_set_output_compression_type(wim,atoi(getenv("WIM_PROGRESS_CODEC")));printf("codec-set %d\n",rc);}
 struct context replacement={.wim=wim,.id=2},ctx={.wim=wim,.id=1,.status=atoi(argv[3]),.stop=atoi(argv[4]),.action=atoi(argv[5]),.replacement=&replacement};
 wimlib_register_progress_function(wim,callback,&ctx);
 if(!strcmp(argv[2],"verify"))rc=wimlib_verify_wim(wim,0);
 else if(!strcmp(argv[2],"split"))rc=wimlib_split(wim,"/tmp/wim-progress-part.swm",1,argc==7?atoi(argv[6]):WIMLIB_WRITE_FLAG_RETAIN_GUID|WIMLIB_WRITE_FLAG_NOT_PIPABLE);
 else if(!strcmp(argv[2],"write"))rc=wimlib_write(wim,"/tmp/wim-progress-written.wim",WIMLIB_ALL_IMAGES,argc==7?atoi(argv[6]):WIMLIB_WRITE_FLAG_RETAIN_GUID|WIMLIB_WRITE_FLAG_CHECK_INTEGRITY,1);

#ifdef PROBE_JOIN_PROGRESS
 else if(!strcmp(argv[2],"join")){const char *parts[]={"/tmp/wim-progress-part.swm","/tmp/wim-progress-part2.swm","/tmp/wim-progress-part3.swm"};rc=wimlib_join_with_progress(parts,3,"/tmp/wim-progress-joined.wim",getenv("WIM_PROGRESS_OPEN_FLAGS")?atoi(getenv("WIM_PROGRESS_OPEN_FLAGS")):0,argc==7?atoi(argv[6]):WIMLIB_WRITE_FLAG_CHECK_INTEGRITY,callback,&ctx);}
#endif
 else return 3;
 printf("result %d\n",rc);if(getenv("WIM_PROGRESS_STATE")){struct wimlib_wim_info state;wimlib_get_wim_info(wim,&state);printf("state-total %llu\n",(unsigned long long)state.total_bytes);printf("state-result %d\n",wimlib_iterate_lookup_table(wim,0,state_resource,NULL));}wimlib_register_progress_function(wim,NULL,NULL);wimlib_free(wim);return 0;
}
