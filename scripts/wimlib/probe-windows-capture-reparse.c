/* Original Windows header, real Win32 source tree, no replacement declarations. */
#include <windows.h>
#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <wchar.h>

typedef int (__cdecl *create_fn)(int,WIMStruct **);
typedef int (__cdecl *add_fn)(WIMStruct *,const wchar_t *,const wchar_t *,const wchar_t *,int);
typedef int (__cdecl *write_fn)(WIMStruct *,const wchar_t *,int,int,unsigned);
typedef int (__cdecl *lookup_fn)(WIMStruct *,int,wimlib_iterate_lookup_table_callback_t,void *);
typedef int (__cdecl *tree_fn)(WIMStruct *,int,const wchar_t *,int,wimlib_iterate_dir_tree_callback_t,void *);
typedef void (__cdecl *register_fn)(WIMStruct *,wimlib_progress_func_t,void *);
typedef void (__cdecl *free_fn)(WIMStruct *);
typedef int (__cdecl *info_fn)(WIMStruct *,struct wimlib_wim_info *);
typedef int (__cdecl *init_fn)(int);
static int stop_msg, stop_value;
static void wide(const wchar_t *text) {
    if(!text){printf("null");return;}
    for(;*text;text++)printf("%04x",(unsigned short)*text);
}
static enum wimlib_progress_status progress(enum wimlib_progress_msg msg,
                                          union wimlib_progress_info *info,void *ctx) {
    (void)ctx;
    if(msg==9||msg==10||msg==11) {
        printf("scan %d %llu %llu %llu ",msg,
               (unsigned long long)info->scan.num_dirs_scanned,
               (unsigned long long)info->scan.num_nondirs_scanned,
               (unsigned long long)info->scan.num_bytes_scanned);
        if(msg==10){printf("%d ",info->scan.status);wide(info->scan.cur_path);}
        else if(msg==9) wide(info->scan.wim_target_path); else printf("end-target-omitted");
        printf(" target ");wide(msg==10 && (info->scan.status==3 || info->scan.status==4) ? info->scan.symlink_target : NULL);
        putchar('\n');
    }else printf("progress %d\n",msg);
    return (int)msg==stop_msg ? (enum wimlib_progress_status)stop_value : WIMLIB_PROGRESS_STATUS_CONTINUE;
}
static int blob(const struct wimlib_resource_entry *r,void *ctx) {
    (void)ctx;printf("blob %u %llu %u ",r->is_metadata,(unsigned long long)r->uncompressed_size,r->reference_count);
    if(!r->is_metadata)for(int i=0;i<20;i++)printf("%02x",r->sha1_hash[i]);
    putchar('\n');return 0;
}
static int node(const struct wimlib_dir_entry *d,void *ctx) {
    (void)ctx;printf("node ");wide(d->full_path);
    printf(" %u %u %u %u\n",d->attributes,d->num_links,d->num_named_streams,d->reparse_tag);
    printf("dos ");wide(d->dos_name);putchar('\n');
    printf("sd %llu ",(unsigned long long)d->security_descriptor_size);
    for(size_t i=0;i<d->security_descriptor_size;i++)printf("%02x",(unsigned char)d->security_descriptor[i]);
    putchar('\n');
    for(unsigned i=0;i<=d->num_named_streams;i++){printf("stream ");wide(d->streams[i].stream_name);printf(" %llu ",(unsigned long long)d->streams[i].resource.uncompressed_size);for(int k=0;k<20;k++)printf("%02x",d->streams[i].resource.sha1_hash[k]);putchar('\n');}
    return 0;
}
int wmain(int argc,wchar_t **argv) {
    setvbuf(stdout,NULL,_IONBF,0);
    if(argc!=8&&argc!=9)return 2;
    HMODULE dll=LoadLibraryW(argv[1]);if(!dll){printf("loader %lu\n",GetLastError());return 3;}
    create_fn create=(create_fn)(void*)GetProcAddress(dll,"wimlib_create_new_wim");
    add_fn add=(add_fn)(void*)GetProcAddress(dll,"wimlib_add_image");
    write_fn write=(write_fn)(void*)GetProcAddress(dll,"wimlib_write");
    free_fn release=(free_fn)(void*)GetProcAddress(dll,"wimlib_free");
    info_fn info=(info_fn)(void*)GetProcAddress(dll,"wimlib_get_wim_info");
    register_fn reg=(register_fn)(void*)GetProcAddress(dll,"wimlib_register_progress_function");
    lookup_fn lookup=(lookup_fn)(void*)GetProcAddress(dll,"wimlib_iterate_lookup_table");
    tree_fn tree=(tree_fn)(void*)GetProcAddress(dll,"wimlib_iterate_dir_tree");
    if(!create||!add||!write||!release||!info||!reg||!lookup||!tree){puts("missing-capture-export");FreeLibrary(dll);return 77;}
    if(argc==9){init_fn init=(init_fn)(void*)GetProcAddress(dll,"wimlib_global_init");if(!init)return 77;printf("init %d\n",init(_wtoi(argv[8])));}
    WIMStruct *w=NULL;int rc=create(0,&w);printf("create %d\n",rc);if(rc)return 4;
    stop_msg=_wtoi(argv[6]);stop_value=_wtoi(argv[7]);reg(w,progress,NULL);
    rc=add(w,argv[2],L"Capture-\x00e9\x6f22\xd834\xdd1e",wcscmp(argv[5],L"-")==0?NULL:argv[5],_wtoi(argv[4]));
    printf("add %d\n",rc);struct wimlib_wim_info wi;int ir=info(w,&wi);printf("info %d %u %u\n",ir,wi.image_count,wi.has_rpfix);
    reg(w,NULL,NULL);
    if(!rc){puts("before");printf("lookup %d\n",lookup(w,0,blob,NULL));printf("tree %d\n",tree(w,1,NULL,WIMLIB_ITERATE_DIR_TREE_FLAG_RECURSIVE,node,NULL));
        if(wcscmp(argv[3],L"-")!=0){rc=write(w,argv[3],1,0,1);printf("write %d\n",rc);if(!rc){puts("after");printf("lookup %d\n",lookup(w,0,blob,NULL));}}
    }
    release(w);FreeLibrary(dll);return 0;
}
