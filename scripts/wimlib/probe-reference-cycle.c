#include <wimlib.h>
#include <stdio.h>
int main(int argc,char**argv){if(argc!=3)return 2;WIMStruct*a=NULL,*b=NULL;int r=wimlib_open_wim(argv[1],0,&a);if(r)return r;wimlib_create_new_wim(0,&b);printf("gift %d\n",wimlib_export_image(a,1,b,NULL,NULL,WIMLIB_EXPORT_FLAG_GIFT));printf("reference %d\n",wimlib_reference_resources(a,&b,1,0));wimlib_free(b);printf("verify %d\n",wimlib_verify_wim(a,0));printf("write %d\n",wimlib_write(a,argv[2],-1,0,1));wimlib_free(a);return 0;}
