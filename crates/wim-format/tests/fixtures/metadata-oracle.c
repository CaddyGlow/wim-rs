#include <stdio.h>
#include <wimlib.h>
static int visitor(const struct wimlib_dir_entry *d, void *ctx) {
 (void)ctx;
 printf("%s attrs=%u links=%u streams=%u\n", d->full_path, d->attributes, d->num_links, d->num_named_streams);
 return 0;
}
int main(int argc, char **argv) {
 WIMStruct *w; int ret = wimlib_open_wim(argv[1],0,&w);
 if(ret) return ret;
 ret=wimlib_iterate_dir_tree(w,1,"/",WIMLIB_ITERATE_DIR_TREE_FLAG_RECURSIVE,visitor,NULL);
 printf("status=%d\n",ret); wimlib_free(w); return ret;
}
