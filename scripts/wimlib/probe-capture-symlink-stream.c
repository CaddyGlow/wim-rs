/* Original-header real capture/write stream serialization probe. */
#include <wimlib.h>
#include <stdio.h>
int main(int argc,char **argv) {
 if(argc!=3)return 2;
 WIMStruct *w=NULL; int rc=wimlib_create_new_wim(0,&w);
 if(!rc)rc=wimlib_add_image(w,argv[1],NULL,NULL,WIMLIB_ADD_FLAG_UNIX_DATA|WIMLIB_ADD_FLAG_NORPFIX);
 printf("capture=%d\n",rc);
 if(!rc)rc=wimlib_write(w,argv[2],WIMLIB_ALL_IMAGES,0,1);
 printf("write=%d\n",rc);wimlib_free(w);return rc!=0;
}
