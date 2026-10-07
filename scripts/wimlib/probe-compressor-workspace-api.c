/* SPDX-License-Identifier: LGPL-2.1-or-later */
/* Unchanged upstream public header supplies every ABI function signature. */
#include <wimlib.h>
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define LOAD(lib, name) __typeof__(&wimlib_##name) name = (__typeof__(&wimlib_##name))dlsym(lib, "wimlib_" #name); if (!name) return 2
int main(int argc, char **argv) {
 if (argc != 3) return 2;
 void *native = dlopen(argv[1], RTLD_NOW|RTLD_LOCAL), *oracle = dlopen(argv[2], RTLD_NOW|RTLD_LOCAL);
 if (!native || !oracle) { fprintf(stderr,"%s\n",dlerror()); return 2; }
 LOAD(native, create_compressor); LOAD(native, compress); LOAD(native, free_compressor); LOAD(native,set_default_compression_level);
 LOAD(oracle,create_decompressor); LOAD(oracle,decompress); LOAD(oracle,free_decompressor);
 __typeof__(&wimlib_create_compressor) original_create = (__typeof__(&wimlib_create_compressor))dlsym(oracle,"wimlib_create_compressor");
 __typeof__(&wimlib_free_compressor) original_free = (__typeof__(&wimlib_free_compressor))dlsym(oracle,"wimlib_free_compressor");
 unsigned factories=0;
 for(int codec=-1;codec<=4;codec++) for(unsigned li=0;li<5;li++) for(unsigned si=0;si<4;si++) {
  unsigned levels[]={0,1,50,0x80000032,0x1000000}; size_t sizes[]={0,1,4096,65537};
  struct wimlib_compressor *a=(void*)1,*b=(void*)1;
  int ar=create_compressor(codec,sizes[si],levels[li],&a),br=original_create(codec,sizes[si],levels[li],&b);
  if(ar!=br || (ar && (a!=(void*)1 || b!=(void*)1))) return 12;
  if(!ar) { free_compressor(a); original_free(b); } factories++;
 }
 unsigned char input[4096], saved[4096], output[8192], decoded[4096];
 unsigned cases=0;
 for(int codec=1;codec<=3;codec++) for(unsigned mode=0;mode<2;mode++) for(unsigned levelidx=0;levelidx<5;levelidx++) {
  unsigned levels[]={0,1,50,100,0x80000032};
  for(unsigned i=0;i<sizeof(input);i++) input[i]= mode ? (unsigned char)((i*17+i/23)%251) : 'a';
  memcpy(saved,input,sizeof(input));
  struct wimlib_compressor *c=NULL;
  if(create_compressor(codec,sizeof(input),levels[levelidx],&c)) return 3;
  if(compress(input,0,output,sizeof(output),c)||compress(input,sizeof(input)+1,output,sizeof(output),c)||compress(input,sizeof(input),output,0,c)) return 4;
  struct wimlib_decompressor *d=NULL;
  if(create_decompressor(codec,sizeof(input),&d))return 6;
  for(unsigned repeat=0;repeat<3;repeat++) {
   size_t length=compress(input,sizeof(input),output,sizeof(output),c);
   if(!length||length>sizeof(output))return 5;
   if(decompress(output,length,decoded,sizeof(decoded),d)||memcmp(saved,decoded,sizeof(saved)))return 6;
   if(memcmp(input,saved,sizeof(input)))return 7;
   cases++;
  }
  free_decompressor(d); free_compressor(c);
 }
 free_compressor(NULL);
 if(set_default_compression_level(0,50)!=WIMLIB_ERR_INVALID_COMPRESSION_TYPE || set_default_compression_level(-1,0xFFFFFFFF)) return 8;
 for(int codec=1;codec<=3;codec++) {
  struct wimlib_compressor *c=(void*)1;
  if(create_compressor(codec,0,50,&c)!=WIMLIB_ERR_INVALID_PARAM||c!=(void*)1) return 9;
  if(create_compressor(codec,4096,0x1000000,&c)!=WIMLIB_ERR_INVALID_PARAM||c!=(void*)1) return 10;
  if(create_compressor(codec,4096,50,NULL)!=WIMLIB_ERR_INVALID_PARAM) return 11;
 }
 set_default_compression_level(-1,0);
 printf("{\"native_compressor_original_decoder_cases\":%u,\"level_policy\":\"fixed strategy; tuning incomplete\",\"unchanged_header\":true,\"factory_original_comparisons\":%u}\n",cases,factories);
 return 0;
}
