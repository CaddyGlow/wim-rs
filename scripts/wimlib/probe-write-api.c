/* SPDX-License-Identifier: LGPL-2.1-or-later */
#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
static int blob(const struct wimlib_resource_entry *entry, void *ctx) {
 (void)ctx;
 printf("blob %llu %u %u ",(unsigned long long)entry->uncompressed_size,entry->reference_count,entry->is_metadata);
 for(int i=0;i<20;i++) {printf("%02x",entry->sha1_hash[i]);} puts(""); return 0;
}
int main(int argc,char **argv) {
 if(argc!=8) return 2;
 WIMStruct *wim=NULL; int codec=atoi(argv[4]),flags=atoi(argv[5]),image=atoi(argv[6]),mode=atoi(argv[7]);
 int rc=!strcmp(argv[1],"new")?wimlib_create_new_wim(codec,&wim):wimlib_open_wim(argv[1],0,&wim);
 if(rc) { printf("open %d\n",rc);return 3;}
 if(!strcmp(argv[1],"new")) {int index=0; rc=wimlib_add_empty_image(wim,"empty",&index);if(rc)return 4;}
 struct wimlib_wim_info info;wimlib_get_wim_info(wim,&info);memset(info.guid,0x5a,16);
 info.has_rpfix=1; if(info.image_count)info.boot_index=info.image_count;
 if(wimlib_set_wim_info(wim,&info,WIMLIB_CHANGE_GUID|WIMLIB_CHANGE_RPFIX_FLAG|WIMLIB_CHANGE_BOOT_INDEX))return 5;
 if(info.image_count && wimlib_set_image_name(wim,1,"edited"))return 6;
 if(!strcmp(argv[3],"delete") && wimlib_delete_image(wim,1))return 7;
 if(!strcmp(argv[3],"append")) {int index;if(wimlib_add_empty_image(wim,"added",&index))return 8;}
 if(wimlib_set_output_compression_type(wim,codec))return 9;
 wimlib_get_wim_info(wim,&info);unsigned long long before=(unsigned long long)info.total_bytes;
 if(mode==1) {int fd=open(argv[2],O_CREAT|O_TRUNC|O_RDWR,0600);if(fd<0)return 10;rc=wimlib_write_to_fd(wim,fd,image,flags,1);if(fcntl(fd,F_GETFD)<0)return 11;close(fd);}
 else rc=wimlib_write(wim,argv[2],image,flags,1);
 printf("write %d\n",rc);
 if(!rc) {
  wimlib_get_wim_info(wim,&info);printf("handle %u %u %u %u %u %d %u total_before %llu total_after %llu\n",info.image_count,info.boot_index,info.wim_version,info.chunk_size,info.total_parts,info.compression_type,info.has_rpfix,before,(unsigned long long)info.total_bytes);
  printf("handle_lookup\n");if(wimlib_iterate_lookup_table(wim,0,blob,NULL))return 12;
  WIMStruct *written=NULL;if(wimlib_open_wim(argv[2],0,&written))return 13;
  wimlib_get_wim_info(written,&info);printf("written %u %u %u %u %u %d %u %u %u\n",info.image_count,info.boot_index,info.wim_version,info.chunk_size,info.total_parts,info.compression_type,info.has_rpfix,info.pipable,info.has_integrity_table);
  printf("guid_retained %u\n",!memcmp(info.guid,"ZZZZZZZZZZZZZZZZ",16));
  if(wimlib_verify_wim(written,0))return 14;
  printf("verify 0\n");wimlib_free(written);
 }
 wimlib_free(wim);return 0;
}
