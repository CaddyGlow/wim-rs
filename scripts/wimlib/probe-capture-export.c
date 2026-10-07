#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
int main(int argc,char **argv) {
    if(argc!=5 && argc!=6)return 2;
    WIMStruct *source=NULL,*destination=NULL;
    int status=wimlib_create_new_wim(0,&source);printf("source %d\n",status);if(status)return 0;
    status=wimlib_create_new_wim(0,&destination);printf("destination %d\n",status);if(status){wimlib_free(source);return 0;}
    status=wimlib_add_image(source,argv[1],"Capture",NULL,0);printf("capture %d\n",status);
    if(!status && atoi(argv[3])){char first[4096];snprintf(first,sizeof(first),"%s.first",argv[2]);status=wimlib_write(source,first,WIMLIB_ALL_IMAGES,0,1);printf("first-write %d\n",status);}
    if(!status && argc==6){
        char path[4096];snprintf(path,sizeof(path),"%s/file",argv[1]);
        int mutation=atoi(argv[5]);
        if(mutation==1)unlink(path);
        else {FILE *f=fopen(path,"r+b");if(f){if(mutation==2)ftruncate(fileno(f),1);else fputc('!',f);fclose(f);}}
    }
    if(!status){status=wimlib_export_image(source,1,destination,NULL,NULL,atoi(argv[4]));printf("export %d\n",status);}
    if(argc==6){struct wimlib_wim_info info;wimlib_get_wim_info(destination,&info);printf("images %u\n",info.image_count);}
    if(atoi(argv[4]) & WIMLIB_EXPORT_FLAG_GIFT){wimlib_free(source);source=NULL;}
    if(!status){printf("verify %d\n",wimlib_verify_wim(destination,0));printf("write %d\n",wimlib_write(destination,argv[2],WIMLIB_ALL_IMAGES,0,1));}
    wimlib_free(source);wimlib_free(destination);return 0;
}
