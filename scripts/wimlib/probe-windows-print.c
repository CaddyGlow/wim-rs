/* Real Windows CRT printing through original public function signatures. */
#include <windows.h>
#include <wimlib.h>
#include <stdio.h>
#include <wchar.h>
#include <io.h>
#include <fcntl.h>
#include <locale.h>
typedef int (__cdecl *open_fn)(const wchar_t *,int,WIMStruct **);
typedef int (__cdecl *name_fn)(WIMStruct *,int,const wchar_t *);
typedef int (__cdecl *property_fn)(WIMStruct *,int,const wchar_t *,const wchar_t *);
typedef void (__cdecl *print_header_fn)(const WIMStruct *);
typedef void (__cdecl *print_images_fn)(const WIMStruct *,int);
typedef void (__cdecl *free_fn)(WIMStruct *);
int wmain(int argc,wchar_t **argv) {
    if(argc!=4&&argc!=7)return 2;
    if(argc==7&&!setlocale(LC_ALL,(wcscmp(argv[6],L"french")==0)?"French_France.1252":"C")){fputs("locale-unavailable\n",stderr);return 78;}
    _setmode(_fileno(stdout),wcscmp(argv[3],L"binary")==0?_O_BINARY:_O_TEXT);
    HMODULE dll=LoadLibraryW(argv[1]);if(!dll)return 3;
    open_fn open=(open_fn)GetProcAddress(dll,"wimlib_open_wim");
    name_fn name=(name_fn)GetProcAddress(dll,"wimlib_set_image_name");
    property_fn property=(property_fn)GetProcAddress(dll,"wimlib_set_image_property");
    print_header_fn header=(print_header_fn)GetProcAddress(dll,"wimlib_print_header");
    print_images_fn images=(print_images_fn)GetProcAddress(dll,"wimlib_print_available_images");
    free_fn release=(free_fn)GetProcAddress(dll,"wimlib_free");
    if(!open||!name||!property||!header||!images||!release){fputs("missing-print-export\n",stderr);return 77;}
    WIMStruct *w=NULL;int rc=open(argv[2],0,&w);if(rc)return rc;
    rc=name(w,1,L"Print-\x00e9\x6f22\xd834\xdd1e");if(rc){release(w);return rc;}
    if(argc==7){
        if(wcscmp(argv[5],L"newline")==0){
            rc=name(w,1,L"Print-\n\x00e9\x6f22\xd834\xdd1e");
            if(!rc)rc=property(w,1,L"DESCRIPTION",L"line1\nline2-\x00e9\x6f22");
            if(!rc)rc=property(w,1,L"WINDOWS/LANGUAGES/LANGUAGE[1]",L"fr-FR");
            if(!rc)rc=property(w,1,L"WINDOWS/LANGUAGES/LANGUAGE[2]",L"\x65e5\x672c\x8a9e");
            if(!rc)rc=property(w,1,L"WINDOWS/LANGUAGES/DEFAULT",L"fr-FR");
        }else if(wcscmp(argv[5],L"unpaired")==0){
            const wchar_t bad[]={L'U',0xd800,L'X',0};rc=name(w,1,bad);
        }
        /* 2020-02-03T04:05:06Z in Windows FILETIME, fixed independently of wallclock. */
        if(!rc)rc=property(w,1,L"CREATIONTIME/HIGHPART",L"0x01D5DA47");
        if(!rc)rc=property(w,1,L"CREATIONTIME/LOWPART",L"0x1E1C4500");
        if(!rc)rc=property(w,1,L"LASTMODIFICATIONTIME/HIGHPART",L"0x01D5DA47");
        if(!rc)rc=property(w,1,L"LASTMODIFICATIONTIME/LOWPART",L"0x1E1C4500");
        if(rc){fprintf(stderr,"setter %d\n",rc);release(w);return rc;}
    }
    header(w);images(w,argc==7?_wtoi(argv[4]):WIMLIB_ALL_IMAGES);fflush(stdout);
    release(w);FreeLibrary(dll);return 0;
}
