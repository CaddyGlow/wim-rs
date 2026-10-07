/* Unchanged public Windows header; real image extraction and progress contracts. */
#include <windows.h>
#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <wchar.h>
typedef int (__cdecl *open_fn)(const wchar_t *,int,WIMStruct **);
typedef int (__cdecl *extract_fn)(WIMStruct *,int,const wchar_t *,int);
typedef void (__cdecl *register_fn)(WIMStruct *,wimlib_progress_func_t,void *);
typedef void (__cdecl *free_fn)(WIMStruct *);
static int stop_msg,stop_value;
static const wchar_t *target_arg;
static wchar_t prepared_target[32768];
static void wide(const wchar_t *text){if(!text){printf("null");return;}for(;*text;text++)printf("%04x",(unsigned short)*text);}
static enum wimlib_progress_status progress(enum wimlib_progress_msg msg,union wimlib_progress_info *info,void *ctx){
    (void)ctx;
    printf("event %d",msg);
    if(msg==0||msg==1||msg==3||msg==4||msg==6||msg==7||msg==8){
        printf(" image %u flags %u target-is-argument %d reserved-null %d",info->extract.image,info->extract.extract_flags,
               info->extract.target&&target_arg&&wcscmp(info->extract.target,target_arg)==0,info->extract.reserved==NULL);
        if(msg==3||msg==6)printf(" files %llu %llu",(unsigned long long)info->extract.current_file_count,(unsigned long long)info->extract.end_file_count);
        else printf(" bytes %llu %llu streams %llu %llu",(unsigned long long)info->extract.total_bytes,(unsigned long long)info->extract.completed_bytes,(unsigned long long)info->extract.total_streams,(unsigned long long)info->extract.completed_streams);
        printf(" name ");wide(info->extract.image_name);
    }
    putchar('\n');
    int should_stop=(int)msg==stop_msg;
    if(stop_msg==303||stop_msg==306)should_stop=(int)msg==stop_msg-300&&info->extract.current_file_count>=500;
    return should_stop?(enum wimlib_progress_status)stop_value:WIMLIB_PROGRESS_STATUS_CONTINUE;
}
int wmain(int argc,wchar_t **argv){
    setvbuf(stdout,NULL,_IONBF,0);if(argc!=8)return 2;
    HMODULE dll=LoadLibraryW(argv[1]);if(!dll)return 3;
    open_fn open=(open_fn)GetProcAddress(dll,"wimlib_open_wim");
    extract_fn extract=(extract_fn)GetProcAddress(dll,"wimlib_extract_image");
    register_fn reg=(register_fn)GetProcAddress(dll,"wimlib_register_progress_function");
    free_fn release=(free_fn)GetProcAddress(dll,"wimlib_free");
    if(!open||!extract||!reg||!release)return 77;
    WIMStruct *w=NULL;int rc=open(argv[2],0,&w);printf("open %d\n",rc);if(rc)return 4;
    target_arg=wcscmp(argv[3],L"-null")==0?NULL:wcscmp(argv[3],L"-empty")==0?L"":argv[3];stop_msg=_wtoi(argv[6]);stop_value=_wtoi(argv[7]);
    if(wcsncmp(argv[3],L"-relative:",10)==0){
        wcscpy(prepared_target,argv[3]+10);wchar_t *slash=wcsrchr(prepared_target,L'\\');if(!slash)return 8;*slash=0;
        if(!SetCurrentDirectoryW(prepared_target))return 9;target_arg=slash+1;
    }
    int extended_long=wcsncmp(argv[3],L"-long-extended:",15)==0;
    if(wcsncmp(argv[3],L"-long:",6)==0||extended_long){
        wcscpy(prepared_target,argv[3]+(extended_long?15:6));
        for(int part=0;part<3;part++){
            size_t n=wcslen(prepared_target);prepared_target[n++]=L'\\';for(int k=0;k<100;k++)prepared_target[n++]=(wchar_t)(L'a'+part);prepared_target[n]=0;
            wchar_t extended[32768];wcscpy(extended,L"\\\\?\\");wcscat(extended,prepared_target);
            if(!CreateDirectoryW(extended,NULL)&&GetLastError()!=ERROR_ALREADY_EXISTS)return 10;
        }
        wcscat(prepared_target,L"\\target");target_arg=prepared_target;
        if(extended_long){size_t n=wcslen(prepared_target);wmemmove(prepared_target+4,prepared_target,n+1);wmemcpy(prepared_target,L"\\\\?\\",4);}
    }
    reg(w,progress,NULL);SetLastError(0);rc=extract(w,_wtoi(argv[5]),target_arg,_wtoi(argv[4]));
    printf("extract %d\n",rc);release(w);FreeLibrary(dll);return 0;
}
