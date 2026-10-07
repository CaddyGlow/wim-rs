/* Independent original Windows library verifies and applies actual written WIMs. */
#include <windows.h>
#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
typedef int (__cdecl *open_fn)(const wchar_t *,int,WIMStruct **);
typedef int (__cdecl *verify_fn)(WIMStruct *,int);
typedef int (__cdecl *extract_fn)(WIMStruct *,int,const wchar_t *,int);
typedef void (__cdecl *free_fn)(WIMStruct *);
int wmain(int argc,wchar_t **argv) {
    if(argc!=5)return 2;
    HMODULE dll=LoadLibraryW(argv[1]);if(!dll)return 3;
    open_fn open=(open_fn)GetProcAddress(dll,"wimlib_open_wim");
    verify_fn verify=(verify_fn)GetProcAddress(dll,"wimlib_verify_wim");
    extract_fn extract=(extract_fn)GetProcAddress(dll,"wimlib_extract_image");
    free_fn release=(free_fn)GetProcAddress(dll,"wimlib_free");
    if(!open||!verify||!extract||!release)return 77;
    WIMStruct *w=NULL;int rc=open(argv[2],0,&w);printf("open %d\n",rc);
    if(!rc){rc=verify(w,0);printf("verify %d\n",rc);}
    if(!rc){rc=extract(w,1,argv[3],_wtoi(argv[4]));printf("apply %d\n",rc);}
    if(w)release(w);FreeLibrary(dll);return rc;
}
