#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
#include <fcntl.h>
#include <unistd.h>
#include <string.h>
#include <dirent.h>
static int references(const char *path) { DIR *d=opendir("/proc/self/fd"); int n=0; struct dirent *e; char name[256],target[4096]; while((e=readdir(d))) { snprintf(name,sizeof(name),"/proc/self/fd/%s",e->d_name); ssize_t k=readlink(name,target,sizeof(target)-1); if(k>=0) {target[k]=0;if(!strcmp(target,path))n++;} } closedir(d); return n; }
static void dump(FILE *f) { fflush(f); rewind(f); int c; while ((c=fgetc(f))!=EOF) printf("%02x",(unsigned)c); puts(""); }
static void missing(void) { char *p=(char*)1; size_t n=7; errno=0; int r=wimlib_load_text_file("/native-diagnostic-file-that-does-not-exist",&p,&n); printf("missing=%d,%d,%d,%zu\n",r,errno,p==(char*)1,n); }
int main(int argc,char **argv) {
 int flags=argc>1?atoi(argv[1]):0;
 printf("init=%d\n",wimlib_global_init(flags));
 printf("again=%d\n",wimlib_global_init(-1));
 wimlib_set_print_errors(true); missing();
 FILE *a=tmpfile(),*b=tmpfile(); int fd=fileno(a);
 printf("sink=%d\n",wimlib_set_error_file(a)); missing();
 wimlib_set_print_errors(false); missing();
 wimlib_set_print_errors(true);
 WIMStruct *w=NULL; wimlib_create_new_wim(0,&w); wimlib_add_empty_image(w,"one",NULL);
 printf("path=%d\n",wimlib_set_image_property(w,1,"bad space","x"));
 printf("value=%d\n",wimlib_set_image_property(w,1,"NAME","\001"));
 printf("syntax=%d\n",wimlib_set_image_property(w,1,"NAME[no]","x"));
 errno=0; printf("failed=%d\n",wimlib_set_error_file_by_name("/native-diagnostic-missing-directory/log"));
 missing();
 wimlib_set_error_file(b); printf("borrowed=%d\n",fcntl(fd,F_GETFD)>=0);
 printf("a="); dump(a); missing();
 wimlib_global_cleanup(); printf("b-live=%d\n",fcntl(fileno(b),F_GETFD)>=0); missing();
 printf("b="); dump(b);
 printf("reinit=%d\n",wimlib_global_init(0)); wimlib_set_error_file(NULL); wimlib_set_print_errors(false);
 wimlib_free(w); fclose(a); fclose(b); wimlib_global_cleanup();
 char path[]="/tmp/wim-diagnostic-owned-XXXXXX"; int named=mkstemp(path); write(named,"seed",4);close(named);
 printf("named=%d\n",wimlib_set_error_file_by_name(path)); printf("owned=%d\n",references(path));
 wimlib_global_cleanup(); printf("preinit-cleanup=%d\n",references(path)); missing();
 wimlib_global_init(0); wimlib_global_cleanup(); printf("owned-cleanup=%d\n",references(path));
 FILE *log=fopen(path,"r"); printf("log=");dump(log); fclose(log);
 wimlib_set_error_file_by_name(path); printf("owned-again=%d\n",references(path));
 wimlib_set_error_file(NULL); printf("owned-replace=%d\n",references(path));unlink(path);
 wimlib_set_error_file(stdout); printf("buffered-before-error");missing();
 FILE *full=fopen("/dev/full","w"); wimlib_set_error_file(full); missing();printf("sink-error=%d\n",ferror(full)!=0);wimlib_set_error_file(NULL);fclose(full);
 return 0;
}
