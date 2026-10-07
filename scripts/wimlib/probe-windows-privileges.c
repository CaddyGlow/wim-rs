/* Actual process-token state around unchanged-header global lifecycle APIs. */
#include <windows.h>
#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
typedef int (__cdecl *init_fn)(int);
typedef void (__cdecl *cleanup_fn)(void);
static const wchar_t *names[]={SE_BACKUP_NAME,SE_SECURITY_NAME,SE_RESTORE_NAME,SE_TAKE_OWNERSHIP_NAME,SE_MANAGE_VOLUME_NAME};
static void dump(const char *stage) {
    HANDLE token;DWORD length=0;
    if(!OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&token))exit(4);
    GetTokenInformation(token,TokenPrivileges,NULL,0,&length);
    TOKEN_PRIVILEGES *data=malloc(length);if(!data)exit(5);
    if(!GetTokenInformation(token,TokenPrivileges,data,length,&length))exit(6);
    printf("%s",stage);
    for(unsigned i=0;i<5;i++) {
        LUID wanted;if(!LookupPrivilegeValueW(NULL,names[i],&wanted))exit(7);
        DWORD attributes=0xffffffff;
        for(DWORD j=0;j<data->PrivilegeCount;j++)if(data->Privileges[j].Luid.LowPart==wanted.LowPart&&data->Privileges[j].Luid.HighPart==wanted.HighPart)attributes=data->Privileges[j].Attributes;
        printf(" %lu",attributes);
    }
    putchar('\n');free(data);CloseHandle(token);
}
static void enable_backup(void) {
    HANDLE token;TOKEN_PRIVILEGES data;
    if(!OpenProcessToken(GetCurrentProcess(),TOKEN_ADJUST_PRIVILEGES|TOKEN_QUERY,&token))exit(8);
    data.PrivilegeCount=1;if(!LookupPrivilegeValueW(NULL,SE_BACKUP_NAME,&data.Privileges[0].Luid))exit(9);
    data.Privileges[0].Attributes=SE_PRIVILEGE_ENABLED;
    SetLastError(0);if(!AdjustTokenPrivileges(token,FALSE,&data,0,NULL,NULL)||GetLastError())exit(10);
    CloseHandle(token);
}
int wmain(int argc,wchar_t **argv) {
    if(argc!=4)return 2;HMODULE dll=LoadLibraryW(argv[1]);if(!dll)return 3;
    init_fn init=(init_fn)GetProcAddress(dll,"wimlib_global_init");
    cleanup_fn cleanup=(cleanup_fn)GetProcAddress(dll,"wimlib_global_cleanup");
    if(!init||!cleanup)return 77;
    dump("before");printf("init %d\n",init(_wtoi(argv[2])));dump("initialized");cleanup();dump("cleaned");
    if(_wtoi(argv[3])){enable_backup();dump("external-backup");printf("second-init %d\n",init(WIMLIB_INIT_FLAG_DONT_ACQUIRE_PRIVILEGES));dump("second-initialized");cleanup();dump("second-cleaned");}
    FreeLibrary(dll);return 0;
}
