/* Unchanged public header; isolated owned Windows hardlink/ACL fixture. */
#include <windows.h>
#include <sddl.h>
#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

typedef int (__cdecl *create_fn)(int,WIMStruct **);
typedef int (__cdecl *empty_fn)(WIMStruct *,const wchar_t *,int *);
typedef int (__cdecl *update_fn)(WIMStruct *,int,const struct wimlib_update_command *,size_t,int);
typedef int (__cdecl *lookup_fn)(WIMStruct *,int,wimlib_iterate_lookup_table_callback_t,void *);
typedef int (__cdecl *tree_fn)(WIMStruct *,int,const wchar_t *,int,wimlib_iterate_dir_tree_callback_t,void *);
typedef int (__cdecl *write_fn)(WIMStruct *,const wchar_t *,int,int,unsigned);
typedef void (__cdecl *register_fn)(WIMStruct *,wimlib_progress_func_t,void *);
typedef void (__cdecl *free_fn)(WIMStruct *);
#define LOAD(name,type,symbol) type name; do { FARPROC address=GetProcAddress(dll,symbol); _Static_assert(sizeof(name)==sizeof(address),"Windows function pointer size"); memcpy(&name,&address,sizeof(name)); } while(0)
static const wchar_t root[]=L"C:\\wim-capture-multi-20261003";
static wchar_t first[]=L"C:\\wim-capture-multi-20261003\\first.bin";
static wchar_t alias[]=L"C:\\wim-capture-multi-20261003\\alias.bin";
static int scenario,ends,mutation_failed;
static int set_acl(int changed) {
    PSECURITY_DESCRIPTOR sd=NULL;
    const wchar_t *sddl=changed ? L"O:SYG:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;WD)(D;;FW;;;BU)"
        : L"O:SYG:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;WD)";
    if(!ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl,SDDL_REVISION_1,&sd,NULL))return 0;
    int ok=SetFileSecurityW(first,OWNER_SECURITY_INFORMATION|GROUP_SECURITY_INFORMATION|DACL_SECURITY_INFORMATION,sd);
    LocalFree(sd);return ok;
}
static int fixture(void) {
    if(!CreateDirectoryW(root,NULL)&&GetLastError()!=ERROR_ALREADY_EXISTS)return 0;
    DeleteFileW(alias);
    HANDLE f=CreateFileW(first,GENERIC_WRITE,7,NULL,CREATE_ALWAYS,FILE_ATTRIBUTE_NORMAL,NULL);
    if(f==INVALID_HANDLE_VALUE)return 0;
    DWORD n=0;int ok=WriteFile(f,"data",4,&n,NULL)&&n==4;CloseHandle(f);
    return ok&&CreateHardLinkW(alias,first,NULL)&&set_acl(0);
}
static enum wimlib_progress_status progress(enum wimlib_progress_msg msg,
        union wimlib_progress_info *info,void *context) {
    (void)info;(void)context;
    if(msg==WIMLIB_PROGRESS_MSG_SCAN_END&&++ends==1&&scenario==2){
        int ok=set_acl(1);printf("mutation %d\n",ok);mutation_failed=!ok;
        if(!ok)return WIMLIB_PROGRESS_STATUS_ABORT;
    }
    return WIMLIB_PROGRESS_STATUS_CONTINUE;
}
static void wide(const wchar_t *text){if(!text){printf("null");return;}for(;*text;text++)printf("%04x",(unsigned short)*text);}
static int resource(const struct wimlib_resource_entry *entry,void *context) {
    (void)context;
    printf("resource %u %llu %u\n",entry->is_metadata,
        (unsigned long long)entry->uncompressed_size,entry->reference_count);
    return 0;
}
static int node(const struct wimlib_dir_entry *entry,void *context) {
    (void)context;printf("node ");wide(entry->full_path);printf(" links %u sd %llu ",entry->num_links,(unsigned long long)entry->security_descriptor_size);
    for(size_t i=0;i<entry->security_descriptor_size;i++)printf("%02x",(unsigned char)entry->security_descriptor[i]);
    putchar('\n');return 0;
}
int wmain(int argc,wchar_t **argv) {
    setvbuf(stdout,NULL,_IONBF,0);
    if(argc!=4)return 2;
    scenario=_wtoi(argv[2]);if(scenario<0||scenario>4)return 2;
    HMODULE dll=LoadLibraryW(argv[1]);if(!dll){printf("loader %lu\n",GetLastError());return 3;}
    LOAD(create,create_fn,"wimlib_create_new_wim");
    LOAD(empty,empty_fn,"wimlib_add_empty_image");
    LOAD(update,update_fn,"wimlib_update_image");
    LOAD(lookup,lookup_fn,"wimlib_iterate_lookup_table");
    LOAD(tree,tree_fn,"wimlib_iterate_dir_tree");
    LOAD(write,write_fn,"wimlib_write");
    LOAD(reg,register_fn,"wimlib_register_progress_function");
    LOAD(release,free_fn,"wimlib_free");
    if(!create||!empty||!update||!tree||!lookup||!write||!reg||!release)return 77;
    WIMStruct *w=NULL;int rc=create(0,&w);printf("create %d\n",rc);if(rc)return 4;
    if(!fixture()){printf("fixture-error %lu\n",GetLastError());release(w);return 5;}
    int image=0;rc=empty(w,L"multi",&image);printf("empty %d %d\n",rc,image);if(rc){release(w);return 6;}
    struct wimlib_update_command cmds[2]={0};
    cmds[0].op=cmds[1].op=WIMLIB_UPDATE_OP_ADD;
    cmds[0].add.fs_source_path=first;cmds[0].add.wim_target_path=L"\\a";
    cmds[1].add.fs_source_path=alias;cmds[1].add.wim_target_path=L"\\b";
    if(scenario==0||scenario==3)cmds[1].add.add_flags=WIMLIB_ADD_FLAG_NO_ACLS;
    if(scenario==1)cmds[0].add.add_flags=WIMLIB_ADD_FLAG_NO_ACLS;
    reg(w,progress,NULL);
    if(scenario<3){rc=update(w,image,cmds,2,0);printf("update %d\n",rc);}
    else {
        rc=update(w,image,&cmds[0],1,0);printf("update-first %d\n",rc);
        if(!rc&&scenario==4){int ok=set_acl(1);printf("mutation %d\n",ok);mutation_failed=!ok;}
        if(!rc&&!mutation_failed){rc=update(w,image,&cmds[1],1,0);printf("update-second %d\n",rc);}
    }
    reg(w,NULL,NULL);
    if(!rc&&!mutation_failed){printf("tree %d\n",tree(w,image,NULL,WIMLIB_ITERATE_DIR_TREE_FLAG_RECURSIVE,node,NULL));
        if(wcscmp(argv[3],L"-")!=0){
            rc=write(w,argv[3],image,0,1);printf("write %d\n",rc);
            if(!rc)printf("lookup %d\n",lookup(w,0,resource,NULL));
        }}
    release(w);FreeLibrary(dll);return mutation_failed?7:0;
}
