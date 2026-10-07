/* Unchanged-header capture graph inspection before any stream hashing. */
#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
static int stop, calls;
static void resource(const struct wimlib_resource_entry *r) {
    printf(" %llu %llu %llu %u %u %u%u%u%u%u%u ",
           (unsigned long long)r->uncompressed_size,
           (unsigned long long)r->compressed_size,(unsigned long long)r->offset,
           r->part_number,r->reference_count,r->is_compressed,r->is_metadata,
           r->is_free,r->is_spanned,r->is_missing,r->packed);
    for(int i=0;i<20;i++) printf("%02x",r->sha1_hash[i]);
    putchar('\n');
}
static int blob(const struct wimlib_resource_entry *r,void *context) {
    (void)context;printf("blob");resource(r);
    return ++calls==stop ? -123 : 0;
}
static int node(const struct wimlib_dir_entry *d,void *context) {
    (void)context;printf("node %s %u %u\n",d->full_path,d->num_links,d->num_named_streams);
    for(unsigned i=0;i<=d->num_named_streams;i++) {printf("stream");resource(&d->streams[i].resource);}
    return ++calls==stop ? -123 : 0;
}
int main(int argc,char **argv) {
    if(argc!=4)return 2;
    WIMStruct *w=NULL;int result=wimlib_create_new_wim(0,&w);
    printf("create %d\n",result);if(result)return 0;
    result=wimlib_add_image(w,argv[1],"Inspect",NULL,atoi(argv[2]));
    printf("add %d\n",result);if(result){wimlib_free(w);return 0;}
    stop=atoi(argv[3]);calls=0;
    printf("lookup %d\n",wimlib_iterate_lookup_table(w,0,blob,NULL));
    calls=0;
    printf("tree %d\n",wimlib_iterate_dir_tree(w,1,NULL,WIMLIB_ITERATE_DIR_TREE_FLAG_RECURSIVE,node,NULL));
    wimlib_free(w);return 0;
}
