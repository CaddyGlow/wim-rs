/* Exercise the public wide Windows extraction ABI with original and Rust DLLs. */
#include <windows.h>
#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <wchar.h>
typedef int (__cdecl *open_fn)(const wchar_t *,int,WIMStruct **);
typedef int (__cdecl *image_fn)(WIMStruct *,int,const wchar_t *,int);
typedef int (__cdecl *paths_fn)(WIMStruct *,int,const wchar_t *,const wchar_t * const *,size_t,int);
typedef void (__cdecl *free_fn)(WIMStruct *);
int wmain(int argc,wchar_t **argv){
    if(argc<5)return 2;
    HMODULE dll=LoadLibraryW(argv[1]);if(!dll){printf("loader %lu\n",GetLastError());return 3;}
    open_fn open=(open_fn)GetProcAddress(dll,"wimlib_open_wim");
    image_fn image=(image_fn)GetProcAddress(dll,"wimlib_extract_image");
    paths_fn paths=(paths_fn)GetProcAddress(dll,"wimlib_extract_paths");
    free_fn release=(free_fn)GetProcAddress(dll,"wimlib_free");
    if(!open||!image||!paths||!release)return 77;
    WIMStruct *w=NULL;int rc=open(argv[2],0,&w);printf("open %d\n",rc);if(rc)return 4;
    rc=argc==5?image(w,1,argv[3],_wtoi(argv[4])):
        paths(w,1,argv[3],(const wchar_t *const *)(argv+5),(size_t)(argc-5),_wtoi(argv[4]));
    printf("extract %d\n",rc);release(w);FreeLibrary(dll);return rc?1:0;
}
