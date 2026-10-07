/* SPDX-License-Identifier: LGPL-2.1-or-later */
#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static unsigned done_count;
static enum wimlib_progress_status progress(enum wimlib_progress_msg message, union wimlib_progress_info *info, void *context) {
 (void)info; (void)context;
 if(message==WIMLIB_PROGRESS_MSG_DONE_WITH_FILE)++done_count;
 return WIMLIB_PROGRESS_STATUS_CONTINUE;
}
int main(int argc,char **argv){
 if(argc<2)return 2;
 if(!strcmp(argv[1],"split")){
  if(argc!=7){return 2;}WIMStruct *wim=NULL;int rc=wimlib_open_wim(argv[2],0,&wim);if(rc){printf("open %d\n",rc);return 0;}
  struct wimlib_wim_info info;wimlib_get_wim_info(wim,&info);memset(info.guid,0x5a,16);info.has_rpfix=1;info.boot_index=info.image_count;
  if(wimlib_set_wim_info(wim,&info,WIMLIB_CHANGE_GUID|WIMLIB_CHANGE_RPFIX_FLAG|WIMLIB_CHANGE_BOOT_INDEX))return 3;
  if(info.image_count && wimlib_set_image_name(wim,1,"edited"))return 4;
  if(!strcmp(argv[6],"delete") && info.image_count && wimlib_delete_image(wim,1))return 5;
  if(!strcmp(argv[6],"append")){int image;if(wimlib_add_empty_image(wim,"added",&image))return 6;}
  wimlib_register_progress_function(wim,progress,NULL);
  rc=wimlib_split(wim,argv[3],strtoull(argv[4],NULL,10),atoi(argv[5]));printf("split %d\n",rc);
  wimlib_get_wim_info(wim,&info);printf("handle %u %u %u %u\n",info.image_count,info.boot_index,info.has_rpfix,info.guid[0]);
  printf("done=%u\n",done_count);wimlib_free(wim);return 0;
 }
 if(!strcmp(argv[1],"join")){
  if(argc<6){return 2;}int rc=wimlib_join((const char *const *)(argv+5),(unsigned)(argc-5),argv[2],atoi(argv[3]),atoi(argv[4]));printf("join %d\n",rc);return 0;
 }
 return 2;
}
