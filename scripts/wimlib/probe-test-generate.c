#include "config.h"
#include "wimlib.h"
#include "wimlib/test_support.h"
#include <stdio.h>
#include <stdlib.h>
int main(int argc, char **argv) {
 if(argc!=3)return 2;
 WIMStruct *w=NULL; int ret=wimlib_create_new_wim(0,&w);
 wimlib_seed_random(strtoull(argv[1],NULL,0));
 if(!ret)ret=wimlib_add_image(w,"ignored",NULL,NULL,WIMLIB_ADD_FLAG_GENERATE_TEST_DATA|WIMLIB_ADD_FLAG_NORPFIX);
 if(!ret)ret=wimlib_write(w,argv[2],WIMLIB_ALL_IMAGES,0,0);
 printf("status=%d\n",ret);wimlib_free(w);return ret!=0;
}
