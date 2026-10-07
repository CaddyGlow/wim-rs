/* SPDX-License-Identifier: LGPL-2.1-or-later */
#include <wimlib.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
int main(int argc,char **argv) {
 printf("layout %zu %zu %zu\n",sizeof(struct wimlib_wim_info),offsetof(struct wimlib_wim_info,total_bytes),offsetof(struct wimlib_wim_info,reserved));
 for(int arg=1;arg<argc;arg++) {
  WIMStruct *wim=NULL; int rc=wimlib_open_wim(argv[arg],0,&wim); printf("open %s %d",argv[arg],rc);
  if(!rc) { struct wimlib_wim_info info; memset(&info,0xff,sizeof(info)); if(wimlib_get_wim_info(wim,&info)) return 6; const unsigned char *p=(const void*)&info; putchar(' '); for(size_t i=0;i<sizeof(info);i++) printf("%02x",p[i]); wimlib_free(wim); } puts("");
 }
 unsigned cases=0, setters=0;
 for(int codec=0;codec<4;codec++) for(int mask=-1;mask<=17;mask++) for(unsigned boot=0;boot<2;boot++) for(unsigned bits=0;bits<4;bits++) {
  WIMStruct *wim=NULL; if(wimlib_create_new_wim(codec,&wim)) return 2;
  struct wimlib_wim_info info; memset(&info,0,sizeof(info)); memset(info.guid,0x5a,16);
  if(wimlib_set_wim_info(wim,&info,WIMLIB_CHANGE_GUID)) return 3;
  memset(info.guid,0xa5,16); info.boot_index=boot; info.has_rpfix=bits&1; info.is_marked_readonly=!!(bits&2);
  int result=wimlib_set_wim_info(wim,&info,mask);
  memset(&info,0xff,sizeof(info)); if(wimlib_get_wim_info(wim,&info)) return 4;
  printf("info %d %d %u %u %d ",codec,mask,boot,bits,result);
  const unsigned char *p=(const void*)&info; for(size_t i=0;i<sizeof(info);i++) printf("%02x",p[i]); puts("");
  wimlib_free(wim); cases++;
 }
 const unsigned chunks[]={0,1,4096,8192,32768,65536,131072,2097152,4194304,67108864,1073741824,2147483648u,~0u};
 for(int codec=0;codec<4;codec++) {
  WIMStruct *wim=NULL; if(wimlib_create_new_wim(codec,&wim)) return 5;
  for(int type=-1;type<=4;type++) {
   printf("type %d %d %d %d\n",codec,type,wimlib_set_output_compression_type(wim,type),wimlib_set_output_pack_compression_type(wim,type)); setters+=2;
   for(size_t i=0;i<sizeof(chunks)/sizeof(chunks[0]);i++) {
    printf("chunk %d %d %u %d %d\n",codec,type,chunks[i],wimlib_set_output_chunk_size(wim,chunks[i]),wimlib_set_output_pack_chunk_size(wim,chunks[i])); setters+=2;
   }
  }
  wimlib_free(wim);
 }
 fprintf(stderr,"%u info cases; %u output-setting returns\n",cases,setters);
 return 0;
}
