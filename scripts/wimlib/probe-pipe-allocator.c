#define _GNU_SOURCE
/* Original unchanged-header ABI caller with real hook failure and post-call IO observation. */
#define main pipe_contract_main
#include "probe-pipe-extract.c"
#undef main
#include <unistd.h>
#include <execinfo.h>
#include <dlfcn.h>
#include <stdint.h>
static unsigned long allocations, failure, live;
static void *tracked[65536];
static size_t tracked_count;
static void trace_failure(size_t size) {
    if (!getenv("PIPE_ALLOCATOR_TRACE")) return;
    void *frames[24]; int count=backtrace(frames,24);
    fprintf(stderr,"failure %lu size %zu\n",allocations,size);
    for (int i=0;i<count;i++) {
        Dl_info info;
        if(dladdr(frames[i],&info)) fprintf(stderr,"trace %s+0x%lx %s\n",info.dli_fname,(unsigned long)((uintptr_t)frames[i]-(uintptr_t)info.dli_fbase),info.dli_sname?info.dli_sname:"?");
    }
}
static void *allocate(size_t size) {
    allocations++;
    if (failure && allocations == failure) { trace_failure(size); return NULL; }
    void *pointer = malloc(size);
    if (pointer && tracked_count < sizeof(tracked) / sizeof(tracked[0])) {
        tracked[tracked_count++] = pointer;
        live++;
    }
    return pointer;
}
static void release(void *pointer) {
    if (pointer) for (size_t i=0;i<tracked_count;i++) if (tracked[i]==pointer) {
        tracked[i]=NULL; live--; break;
    }
    free(pointer);
}
static void *resize(void *pointer,size_t size) {
    allocations++;
    if (failure && allocations == failure) { trace_failure(size); return NULL; }
    size_t index=tracked_count;
    for (size_t i=0;i<tracked_count;i++) if (tracked[i]==pointer && pointer) { index=i; break; }
    void *replacement=realloc(pointer,size);
    if (replacement) {
        if (index<tracked_count) tracked[index]=replacement;
        else if (tracked_count<sizeof(tracked)/sizeof(tracked[0])) { tracked[tracked_count++]=replacement; live++; }
    }
    return replacement;
}
int main(int argc,char **argv) {
    if(argc!=6)return 2;
    failure=strtoul(argv[5],NULL,10);
    int observer=dup(0);
    if(observer<0)return 3;
    wimlib_set_memory_allocator(allocate,release,resize);
    int result=pipe_contract_main(5,argv);
    unsigned long long unread=0;
    char bytes[4096]; ssize_t count;
    while((count=read(observer,bytes,sizeof(bytes)))>0)unread+=(unsigned long long)count;
    close(observer);
    const char *size=getenv("PIPE_FIXTURE_SIZE");
    if(count<0 || !size)return 4;
    printf("api-read %llu\n",strtoull(size,NULL,10)-unread);
    printf("allocator %lu live %lu\n",allocations,live);
    wimlib_global_cleanup();
    printf("cleanup-live %lu\n",live);
    return result;
}
