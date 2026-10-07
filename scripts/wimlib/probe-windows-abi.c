/* Actual Windows caller compiled against the unchanged original public header.
 * No wimlib import library is needed: both original/native DLLs use LoadLibraryW.
 * Run: probe-windows-abi.exe DLL [fixture.wim] [disposable-output-directory]
 */
#ifndef _WIN32
#error This probe must be built for Windows; Linux layouts are not Windows evidence.
#endif
#ifdef __MINGW32__
#include <windows.h>
#else
#include <Windows.h>
#endif
#include <wimlib.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

#define ABI_VALUES(X) \
 X(pointer_size, sizeof(void *)) \
 X(size_t_size, sizeof(size_t)) \
 X(long_size, sizeof(long)) \
 X(tchar_size, sizeof(wimlib_tchar)) \
 X(timespec_size, sizeof(struct wimlib_timespec)) \
 X(timespec_nsec_offset, offsetof(struct wimlib_timespec,tv_nsec)) \
 X(timespec_nsec_size, sizeof(((struct wimlib_timespec *)0)->tv_nsec)) \
 X(wim_info_size, sizeof(struct wimlib_wim_info)) \
 X(wim_info_total_bytes_offset, offsetof(struct wimlib_wim_info,total_bytes)) \
 X(wim_info_reserved_offset, offsetof(struct wimlib_wim_info,reserved)) \
 X(resource_entry_size, sizeof(struct wimlib_resource_entry)) \
 X(resource_hash_offset, offsetof(struct wimlib_resource_entry,sha1_hash)) \
 X(resource_refcount_offset, offsetof(struct wimlib_resource_entry,reference_count)) \
 X(resource_raw_offset, offsetof(struct wimlib_resource_entry,raw_resource_offset_in_wim)) \
 X(stream_entry_size, sizeof(struct wimlib_stream_entry)) \
 X(stream_resource_offset, offsetof(struct wimlib_stream_entry,resource)) \
 X(dir_entry_size, sizeof(struct wimlib_dir_entry)) \
 X(dir_depth_offset, offsetof(struct wimlib_dir_entry,depth)) \
 X(dir_attributes_offset, offsetof(struct wimlib_dir_entry,attributes)) \
 X(dir_creation_offset, offsetof(struct wimlib_dir_entry,creation_time)) \
 X(dir_write_offset, offsetof(struct wimlib_dir_entry,last_write_time)) \
 X(dir_access_offset, offsetof(struct wimlib_dir_entry,last_access_time)) \
 X(dir_unix_uid_offset, offsetof(struct wimlib_dir_entry,unix_uid)) \
 X(dir_streams_offset, offsetof(struct wimlib_dir_entry,streams)) \
 X(progress_info_size, sizeof(union wimlib_progress_info)) \
 X(progress_extract_size, sizeof(((union wimlib_progress_info *)0)->extract)) \
 X(progress_extract_bytes_offset, offsetof(union wimlib_progress_info,extract.total_bytes)) \
 X(progress_extract_guid_offset, offsetof(union wimlib_progress_info,extract.guid)) \
 X(progress_scan_size, sizeof(((union wimlib_progress_info *)0)->scan)) \
 X(progress_scan_bytes_offset, offsetof(union wimlib_progress_info,scan.num_bytes_scanned)) \
 X(progress_write_size, sizeof(((union wimlib_progress_info *)0)->write_streams)) \
 X(progress_write_bytes_offset, offsetof(union wimlib_progress_info,write_streams.total_bytes)) \
 X(progress_unmount_size, sizeof(((union wimlib_progress_info *)0)->unmount)) \
 X(capture_source_size, sizeof(struct wimlib_capture_source)) \
 X(capture_reserved_offset, offsetof(struct wimlib_capture_source,reserved)) \
 X(capture_reserved_size, sizeof(((struct wimlib_capture_source *)0)->reserved)) \
 X(update_command_size, sizeof(struct wimlib_update_command)) \
 X(update_add_offset, offsetof(struct wimlib_update_command,add)) \
 X(update_add_size, sizeof(struct wimlib_add_command)) \
 X(update_delete_size, sizeof(struct wimlib_delete_command)) \
 X(update_rename_size, sizeof(struct wimlib_rename_command)) \
 X(compression_enum_size, sizeof(enum wimlib_compression_type)) \
 X(error_enum_size, sizeof(enum wimlib_error_code)) \
 X(progress_callback_size, sizeof(wimlib_progress_func_t))
#define COUNT_VALUE(name,value) +1
enum { ABI_COUNT = 0 ABI_VALUES(COUNT_VALUE) };
/* This retained record permits compile-only PE inspection without pretending
 * to execute the Windows caller. Runtime output reads the same constants. */
const struct { char magic[16]; uint32_t count; uint32_t values[ABI_COUNT]; } wim_abi_layout = {
    "WIMWINABI64V1", ABI_COUNT,
#define VALUE(name,value) (uint32_t)(value),
    { ABI_VALUES(VALUE) }
};

static void wide(const char *label, const wchar_t *text)
{
    printf("wide %s ", label);
    if (!text) { puts("NULL"); return; }
    unsigned count=0;
    while (text[count] && count<4096) { printf("%04x",(unsigned)(uint16_t)text[count]); ++count; }
    printf(" units %u terminated %d\n",count,text[count]==0);
}
static uint32_t word(const void *value, size_t offset)
{
    uint32_t result; memcpy(&result,(const unsigned char *)value+offset,sizeof(result)); return result;
}
static void layout(void)
{
    unsigned index=0;
#define PRINT(name,value) printf("layout " #name " %u\n",wim_abi_layout.values[index++]);
    ABI_VALUES(PRINT)
    struct wimlib_wim_info info;
#define INFO_BIT(name) do { memset(&info,0,sizeof(info)); info.name=1; printf("info-bit " #name " %08x\n",word(&info,offsetof(struct wimlib_wim_info,reserved)-4)); } while(0)
    INFO_BIT(has_integrity_table); INFO_BIT(opened_from_file); INFO_BIT(is_readonly);
    INFO_BIT(has_rpfix); INFO_BIT(is_marked_readonly); INFO_BIT(spanned);
    INFO_BIT(write_in_progress); INFO_BIT(metadata_only); INFO_BIT(resource_only); INFO_BIT(pipable);
    struct wimlib_resource_entry resource;
#define RESOURCE_BIT(name) do { memset(&resource,0,sizeof(resource)); resource.name=1; printf("resource-bit " #name " %08x\n",word(&resource,offsetof(struct wimlib_resource_entry,reference_count)+4)); } while(0)
    RESOURCE_BIT(is_compressed); RESOURCE_BIT(is_metadata); RESOURCE_BIT(is_free);
    RESOURCE_BIT(is_spanned); RESOURCE_BIT(is_missing); RESOURCE_BIT(packed);
}

/* Function pointer types use the original header's public parameter types. */
typedef const wimlib_tchar *(__cdecl *version_fn)(void);
typedef const wimlib_tchar *(__cdecl *error_fn)(enum wimlib_error_code);
typedef const wimlib_tchar *(__cdecl *compression_fn)(enum wimlib_compression_type);
typedef int (__cdecl *create_fn)(enum wimlib_compression_type,WIMStruct **);
typedef void (__cdecl *free_fn)(WIMStruct *);
typedef int (__cdecl *info_fn)(WIMStruct *,struct wimlib_wim_info *);
typedef int (__cdecl *add_empty_fn)(WIMStruct *,const wimlib_tchar *,int *);
typedef const wimlib_tchar *(__cdecl *name_fn)(const WIMStruct *,int);
typedef int (__cdecl *set_property_fn)(WIMStruct *,int,const wimlib_tchar *,const wimlib_tchar *);
typedef const wimlib_tchar *(__cdecl *property_fn)(const WIMStruct *,int,const wimlib_tchar *);
typedef int (__cdecl *resolve_fn)(WIMStruct *,const wimlib_tchar *);
typedef int (__cdecl *open_fn)(const wimlib_tchar *,int,WIMStruct **);
typedef int (__cdecl *xml_fn)(WIMStruct *,void **,size_t *);
typedef int (__cdecl *extract_xml_fn)(WIMStruct *,FILE *);
typedef int (__cdecl *write_fn)(WIMStruct *,const wimlib_tchar *,int,int,unsigned);
#define LOAD(type,var,symbol) type var=(type)GetProcAddress(dll,#symbol); if(!var){printf("required-missing " #symbol "\n");FreeLibrary(dll);return 6;}

int wmain(int argc, wchar_t **argv)
{
    layout();
    if(argc<2) { puts("runtime not-run"); return 0; }
    HMODULE dll=LoadLibraryW(argv[1]);
    if(!dll){printf("load-error %lu\n",GetLastError());return 5;}
    unsigned exports=0;
#define EXPORT(name) do { int present=GetProcAddress(dll,#name)!=NULL; printf("export " #name " %d\n",present); exports+=(unsigned)present; } while(0)
#include "probe-windows-exports.inc"
    printf("export-count %u\n",exports);
    LOAD(version_fn,version,wimlib_get_version_string)
    LOAD(error_fn,error,wimlib_get_error_string)
    LOAD(compression_fn,compression,wimlib_get_compression_type_string)
    LOAD(create_fn,create,wimlib_create_new_wim)
    LOAD(free_fn,release,wimlib_free)
    LOAD(info_fn,get_info,wimlib_get_wim_info)
    LOAD(add_empty_fn,add_empty,wimlib_add_empty_image)
    LOAD(name_fn,get_name,wimlib_get_image_name)
    LOAD(set_property_fn,set_property,wimlib_set_image_property)
    LOAD(property_fn,get_property,wimlib_get_image_property)
    LOAD(resolve_fn,resolve,wimlib_resolve_image)
    wide("version",version());
    for(int i=-1;i<=4;++i){printf("compression %d\n",i);wide("compression",compression((enum wimlib_compression_type)i));}
    for(int i=-1;i<=94;++i){printf("error %d\n",i);wide("error",error((enum wimlib_error_code)i));}
    WIMStruct *wim=NULL;
    printf("create %d\n",create(WIMLIB_COMPRESSION_TYPE_NONE,&wim));
    if(!wim){FreeLibrary(dll);return 7;}
    struct { unsigned char before[16];struct wimlib_wim_info info;unsigned char after[16]; } guarded;
    memset(&guarded,0xa5,sizeof(guarded));
    int ret=get_info(wim,&guarded.info);
    int canary=1;for(unsigned i=0;i<16;++i)canary&=guarded.before[i]==0xa5&&guarded.after[i]==0xa5;
    printf("info %d canary %d images %u chunk %u part %u/%u compression %d flags %08x\n",ret,canary,guarded.info.image_count,guarded.info.chunk_size,guarded.info.part_number,guarded.info.total_parts,guarded.info.compression_type,word(&guarded.info,offsetof(struct wimlib_wim_info,reserved)-4));
    static const wchar_t image_name[]={L'A',0x00e9,0x6f22,0xd834,0xdd1e,0};
    static const wchar_t property_text[]={L'v',0x00e9,0xd834,0xdd1e,0};
    int image=0;printf("add-empty %d\n",add_empty(wim,image_name,&image));printf("image-index %d\n",image);
    wide("name",get_name(wim,1));printf("resolve %d\n",resolve(wim,image_name));
    printf("set-property %d\n",set_property(wim,1,L"DESCRIPTION",property_text));wide("property",get_property(wim,1,L"DESCRIPTION"));
    if(argc>=4){
        LOAD(write_fn,write_wim,wimlib_write)
        wchar_t output[32768];int length=swprintf(output,32768,L"%ls\\native-A\u00e9\u6f22.wim",argv[3]);
        if(length<0){release(wim);FreeLibrary(dll);return 8;}
        printf("write-wide %d\n",write_wim(wim,output,WIMLIB_ALL_IMAGES,0,1));
    }
    release(wim);release(NULL);
    if(argc>=3){
        LOAD(open_fn,open_wim,wimlib_open_wim)
        wim=NULL;printf("open-wide %d\n",open_wim(argv[2],0,&wim));
        if(wim){
            xml_fn get_xml=(xml_fn)GetProcAddress(dll,"wimlib_get_xml_data");
            extract_xml_fn extract_xml=(extract_xml_fn)GetProcAddress(dll,"wimlib_extract_xml_data");
            if(get_xml&&extract_xml){
                void *xml=NULL;size_t size=0;int result=get_xml(wim,&xml,&size);
                printf("xml-get %d size %zu bom %04x\n",result,size,size>=2?(unsigned)((const uint16_t *)xml)[0]:0);
                FILE *fp=NULL;if(tmpfile_s(&fp)!=0||!fp){release(wim);FreeLibrary(dll);return 9;}
                int extracted=extract_xml(wim,fp);fflush(fp);fseek(fp,0,SEEK_END);long actual=ftell(fp);rewind(fp);
                unsigned char *bytes=malloc(size?size:1);size_t count=fread(bytes,1,size,fp);
                printf("xml-CRT-FILE %d bytes %ld same %d\n",extracted,actual,actual>=0&&(size_t)actual==size&&count==size&&!memcmp(xml,bytes,size));
                fclose(fp);free(bytes);free(xml); /* Actual caller CRT ownership test. */
            }else puts("xml-runtime missing-exports");
            release(wim);
        }
    }
    FreeLibrary(dll);puts("runtime completed");return 0;
}
