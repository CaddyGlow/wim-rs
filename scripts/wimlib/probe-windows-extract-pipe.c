/* Original public header, MSVCRT descriptors, real anonymous Windows pipe. */
#include <windows.h>
#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <fcntl.h>
#include <io.h>
#include <wchar.h>
typedef int (__cdecl *open_fn)(const wchar_t *,int,WIMStruct **);
typedef int (__cdecl *write_fn)(WIMStruct *,const wchar_t *,int,int,unsigned);
typedef void (__cdecl *free_fn)(WIMStruct *);
typedef int (__cdecl *pipe_fn)(int,const wchar_t *,const wchar_t *,int);
typedef int (__cdecl *progress_pipe_fn)(int,const wchar_t *,const wchar_t *,int,wimlib_progress_func_t,void *);
struct feeder {HANDLE source,output;DWORD chunk;unsigned long long written;DWORD error;};
static DWORD WINAPI feed(void *arg){
    struct feeder *f=arg;unsigned char bytes[65536];DWORD n=0;
    while(ReadFile(f->source,bytes,f->chunk,&n,NULL)&&n){
        DWORD offset=0;while(offset<n){DWORD done=0;if(!WriteFile(f->output,bytes+offset,n-offset,&done,NULL)){f->error=GetLastError();goto out;}offset+=done;f->written+=done;}
    }
out:CloseHandle(f->source);CloseHandle(f->output);return 0;
}
static int stop_msg,stop_value;
static enum wimlib_progress_status progress(enum wimlib_progress_msg msg,union wimlib_progress_info *info,void *ctx){
    (void)ctx;printf("event %d",msg);
    if(msg==5)printf(" part %u/%u",info->extract.part_number,info->extract.total_parts);
    if(msg==0||msg==3||msg==4||msg==6||msg==7)printf(" image %u flags %u bytes %llu/%llu streams %llu/%llu files %llu/%llu",info->extract.image,info->extract.extract_flags,(unsigned long long)info->extract.completed_bytes,(unsigned long long)info->extract.total_bytes,(unsigned long long)info->extract.completed_streams,(unsigned long long)info->extract.total_streams,(unsigned long long)info->extract.current_file_count,(unsigned long long)info->extract.end_file_count);
    putchar('\n');return (int)msg==stop_msg?(enum wimlib_progress_status)stop_value:WIMLIB_PROGRESS_STATUS_CONTINUE;
}
int wmain(int argc,wchar_t **argv){
    setvbuf(stdout,NULL,_IONBF,0);if(argc<5)return 2;
    HMODULE dll=LoadLibraryW(argv[1]);if(!dll)return 3;
    if(wcscmp(argv[2],L"generate")==0){
        open_fn open=(open_fn)GetProcAddress(dll,"wimlib_open_wim");write_fn write=(write_fn)GetProcAddress(dll,"wimlib_write");free_fn release=(free_fn)GetProcAddress(dll,"wimlib_free");if(!open||!write||!release)return 77;
        WIMStruct *w=NULL;int rc=open(argv[3],0,&w);printf("open %d\n",rc);if(!rc){rc=write(w,argv[4],1,WIMLIB_WRITE_FLAG_PIPABLE,1);printf("write %d\n",rc);release(w);}return 0;
    }
    if(argc!=11)return 2;
    pipe_fn plain=(pipe_fn)GetProcAddress(dll,"wimlib_extract_image_from_pipe");progress_pipe_fn with_progress=(progress_pipe_fn)GetProcAddress(dll,"wimlib_extract_image_from_pipe_with_progress");if(!plain||!with_progress)return 77;
    HANDLE read_handle,write_handle;if(!CreatePipe(&read_handle,&write_handle,NULL,4096))return 4;
    HANDLE observer;if(!DuplicateHandle(GetCurrentProcess(),read_handle,GetCurrentProcess(),&observer,0,FALSE,DUPLICATE_SAME_ACCESS))return 8;
    struct feeder f={CreateFileW(argv[3],GENERIC_READ,FILE_SHARE_READ,NULL,OPEN_EXISTING,0,NULL),write_handle,(DWORD)_wtoi(argv[7]),0,0};if(f.source==INVALID_HANDLE_VALUE||f.chunk==0||f.chunk>65536)return 5;
    int fd=_open_osfhandle((intptr_t)read_handle,_O_BINARY);if(fd<0)return 6;
    HANDLE thread=CreateThread(NULL,0,feed,&f,0,NULL);if(!thread)return 7;
    stop_msg=_wtoi(argv[8]);stop_value=_wtoi(argv[9]);int rc;
    const wchar_t *image=wcscmp(argv[4],L"-null")==0?NULL:argv[4];
    if(_wtoi(argv[10]))rc=with_progress(fd,image,argv[5],_wtoi(argv[6]),progress,NULL);else rc=plain(fd,image,argv[5],_wtoi(argv[6]));
    printf("extract %d fd-valid %d mode-binary %d\n",rc,_get_osfhandle(fd)!=-1,_setmode(fd,_O_BINARY)==_O_BINARY);
    unsigned long long drained=0;unsigned char buffer[32768];DWORD n;while(ReadFile(observer,buffer,sizeof(buffer),&n,NULL)&&n)drained+=n;
    CloseHandle(observer);
    WaitForSingleObject(thread,INFINITE);CloseHandle(thread);
    printf("written %llu drained %llu api-read %llu feeder-error %lu close %d\n",f.written,drained,f.written-drained,(unsigned long)f.error,_close(fd));
    FreeLibrary(dll);return 0;
}
