/* Linux test-only interposition to exercise fstat/read failures deterministically. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
static int matches(int fd,const char *needle){
    int saved=errno;char link[64],path[1024];snprintf(link,sizeof(link),"/proc/self/fd/%d",fd);
    ssize_t length=readlink(link,path,sizeof(path)-1);int result=0;
    if(length>=0){path[length]=0;result=strstr(path,needle)!=NULL;}
    errno=saved;return result;
}
ssize_t read(int fd,void *buffer,size_t size){
    if(matches(fd,"forced-eof.txt"))return 0;
    if(matches(fd,"forced-read-error.txt")){errno=EIO;return -1;}
    ssize_t (*original)(int,void *,size_t)=dlsym(RTLD_NEXT,"read");return original(fd,buffer,size);
}
int fstat(int fd,struct stat *st){
    if(matches(fd,"forced-stat-error.txt")){errno=EIO;return -1;}
    int (*original)(int,struct stat *)=dlsym(RTLD_NEXT,"fstat");return original(fd,st);
}
int fstat64(int fd,struct stat64 *st){
    if(matches(fd,"forced-stat-error.txt")){errno=EIO;return -1;}
    int (*original)(int,struct stat64 *)=dlsym(RTLD_NEXT,"fstat64");return original(fd,st);
}
int statx(int fd,const char *path,int flags,unsigned mask,struct statx *st){
    if(matches(fd,"forced-stat-error.txt")){errno=EIO;return -1;}
    int (*original)(int,const char *,int,unsigned,struct statx *)=dlsym(RTLD_NEXT,"statx");return original(fd,path,flags,mask,st);
}
