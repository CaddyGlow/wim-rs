/* Unchanged original-header overwrite policy and lifecycle oracle. */
#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
static int abort_event;
static enum wimlib_progress_status progress(enum wimlib_progress_msg message,
 union wimlib_progress_info *info, void *ctx)
{
 (void)ctx;
 printf("event %d", message);
 if (message == WIMLIB_PROGRESS_MSG_WRITE_STREAMS)
  printf(" streams %llu %llu %llu %llu", (unsigned long long)info->write_streams.total_bytes,
   (unsigned long long)info->write_streams.completed_bytes,
   (unsigned long long)info->write_streams.total_streams,
   (unsigned long long)info->write_streams.completed_streams);
 if (message == WIMLIB_PROGRESS_MSG_CALC_INTEGRITY)
  printf(" integrity %llu %llu %u %u", (unsigned long long)info->integrity.total_bytes,
   (unsigned long long)info->integrity.completed_bytes, info->integrity.total_chunks, info->integrity.completed_chunks);
 if (message == WIMLIB_PROGRESS_MSG_RENAME)
  printf(" rename-to %s from-prefix %d", info->rename.to,
   strncmp(info->rename.from, info->rename.to, strlen(info->rename.to)) == 0);
 printf("\n");
 return message == abort_event ? WIMLIB_PROGRESS_STATUS_ABORT : WIMLIB_PROGRESS_STATUS_CONTINUE;
}
int main(int argc, char **argv)
{
 if (argc != 5) return 2;
 WIMStruct *wim = NULL;
 const char *path = argv[1];
 unsigned flags = strtoul(argv[3], NULL, 0);
 abort_event = atoi(argv[4]);
 int status = strcmp(path,"new") == 0 ? wimlib_create_new_wim(0,&wim) : wimlib_open_wim(path,0,&wim);
 printf("open %d\n", status);
 if (status) return 0;
 wimlib_register_progress_function(wim,progress,NULL);
 if (strcmp(argv[2],"xml") == 0) status = wimlib_set_image_property(wim,1,"DESCRIPTION","overwrite oracle");
 else if (strcmp(argv[2],"add") == 0) status = wimlib_add_empty_image(wim,"added",NULL);
 else if (strcmp(argv[2],"delete") == 0) status = wimlib_delete_image(wim,1);
 else if (strcmp(argv[2],"codec") == 0) status = wimlib_set_output_compression_type(wim,0);
 else if (strcmp(argv[2],"invalidxml") == 0) status = wimlib_set_image_property(wim,1,"DESCRIPTION","\xff");
 else if (strcmp(argv[2],"readonly") == 0) {
  struct wimlib_wim_info info; memset(&info,0,sizeof(info)); info.is_readonly = 1;
  status = wimlib_set_wim_info(wim,&info,WIMLIB_CHANGE_READONLY_FLAG);
 }
 printf("mutation %d\n", status);
 printf("overwrite %d\n", wimlib_overwrite(wim,flags,1));
 wimlib_free(wim);
 if (strcmp(path,"new") != 0) {
  struct stat st; if (!stat(path,&st)) printf("size %llu\n",(unsigned long long)st.st_size);
  status = wimlib_open_wim(path,0,&wim); printf("reopen %d\n",status);
  if (!status) { struct wimlib_wim_info info; wimlib_get_wim_info(wim,&info);
   printf("images %u readonly %u\n",info.image_count,info.is_readonly); wimlib_free(wim); }
 }
 return 0;
}
