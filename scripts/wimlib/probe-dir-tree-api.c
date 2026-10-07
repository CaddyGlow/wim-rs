/* Unchanged-header directory callback and flexible-array ABI client. */
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wimlib.h>
static void hex(const void *p, size_t n) { const unsigned char *s=p; if (!p) {printf("NULL");return;} for(size_t i=0;i<n;i++)printf("%02x",s[i]); }
static void text(const char *s) { if (!s)printf("NULL");else hex(s,strlen(s)); }
struct ctx {int calls,stop;};
static int callback(const struct wimlib_dir_entry *d,void *raw) {
    struct ctx *c=raw; ++c->calls;
    printf("entry ");text(d->filename);printf(" ");text(d->dos_name);printf(" ");text(d->full_path);
    printf(" %zu %zu ",d->depth,d->security_descriptor_size);hex(d->security_descriptor,d->security_descriptor_size);
    printf(" %u %u %u %u %llu",d->attributes,d->reparse_tag,d->num_links,d->num_named_streams,(unsigned long long)d->hard_link_group_id);
    printf(" %lld:%ld %lld:%ld %lld:%ld",(long long)d->creation_time.tv_sec,(long)d->creation_time.tv_nsec,(long long)d->last_write_time.tv_sec,(long)d->last_write_time.tv_nsec,(long long)d->last_access_time.tv_sec,(long)d->last_access_time.tv_nsec);
    printf(" %u %u %u %u ",d->unix_uid,d->unix_gid,d->unix_mode,d->unix_rdev);hex(&d->object_id,sizeof(d->object_id));
    printf(" %d %d %d %d",d->creation_time_high,d->last_write_time_high,d->last_access_time_high,d->reserved2);
    for(unsigned i=0;i<4;i++)printf(" %llu",(unsigned long long)d->reserved[i]);putchar('\n');
    for(unsigned i=0;i<=d->num_named_streams;i++) {
        const struct wimlib_stream_entry *s=&d->streams[i];const struct wimlib_resource_entry *r=&s->resource;
        printf("stream ");text(s->stream_name);printf(" %llu %llu %llu ",(unsigned long long)r->uncompressed_size,(unsigned long long)r->compressed_size,(unsigned long long)r->offset);hex(r->sha1_hash,20);
        printf(" %u %u %u%u%u%u%u%u %llu %llu %llu",r->part_number,r->reference_count,r->is_compressed,r->is_metadata,r->is_free,r->is_spanned,r->is_missing,r->packed,(unsigned long long)r->raw_resource_offset_in_wim,(unsigned long long)r->raw_resource_compressed_size,(unsigned long long)r->raw_resource_uncompressed_size);
        for(unsigned j=0;j<4;j++)printf(" %llu",(unsigned long long)s->reserved[j]);putchar('\n');
    }
    return c->stop && c->calls==c->stop ? -123 : 0;
}
#define OFF(field) printf("dir." #field "=%zu\n",offsetof(struct wimlib_dir_entry,field))
int main(int argc,char **argv) {
    if(argc==2 && !strcmp(argv[1],"--layout")) {
        printf("dir.size=%zu\nstream.size=%zu\nstream.resource=%zu\nstream.reserved=%zu\nobject.size=%zu\ntime.size=%zu\ntime.nsec=%zu\n",sizeof(struct wimlib_dir_entry),sizeof(struct wimlib_stream_entry),offsetof(struct wimlib_stream_entry,resource),offsetof(struct wimlib_stream_entry,reserved),sizeof(struct wimlib_object_id),sizeof(struct wimlib_timespec),offsetof(struct wimlib_timespec,tv_nsec));
        OFF(filename);OFF(dos_name);OFF(full_path);OFF(depth);OFF(security_descriptor);OFF(security_descriptor_size);OFF(attributes);OFF(reparse_tag);OFF(num_links);OFF(num_named_streams);OFF(hard_link_group_id);OFF(creation_time);OFF(last_write_time);OFF(last_access_time);OFF(unix_uid);OFF(unix_gid);OFF(unix_mode);OFF(unix_rdev);OFF(object_id);OFF(creation_time_high);OFF(last_write_time_high);OFF(last_access_time_high);OFF(reserved2);OFF(reserved);OFF(streams);return 0;
    }
    if(argc!=6)return 2;
    WIMStruct *w=NULL;int result=wimlib_open_wim(argv[1],0,&w);printf("open=%d\n",result);if(result)return 0;
    struct ctx c={0,atoi(argv[5])};const char *path=!strcmp(argv[2],"@NULL") ? NULL : !strcmp(argv[2],"@WTF8") ? "\xed\xa0\x80" : !strcmp(argv[2],"@BADUTF8") ? "\xff" : argv[2];
    result=wimlib_iterate_dir_tree(w,atoi(argv[4]),path,atoi(argv[3]),callback,&c);
    printf("result=%d calls=%d\n",result,c.calls);wimlib_free(w);return 0;
}
