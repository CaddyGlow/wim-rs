/* Unchanged public-header client: literal callback order and every stop point. */
#include <wimlib.h>
#include <stdio.h>
#include <stdlib.h>
struct state { unsigned count, stop; };
static int row(const struct wimlib_resource_entry *e, void *opaque)
{
    struct state *s = opaque;
    printf("row:%u:", ++s->count);
    for (unsigned i = 0; i < 20; i++) printf("%02x", e->sha1_hash[i]);
    printf(":%llu:%u:%u\n", (unsigned long long)e->uncompressed_size,
           e->reference_count, e->is_metadata);
    return s->stop == s->count ? 37 : 0;
}
int main(int argc, char **argv)
{
    WIMStruct *source = NULL, *wim = NULL;
    if (argc != 3) return 2;
    int ret = wimlib_open_wim(argv[1], 0, &source);
    printf("open:%d\n", ret);
    if (ret) return 0;
    if (atoi(argv[2])) {
        ret = wimlib_create_new_wim(WIMLIB_COMPRESSION_TYPE_NONE, &wim);
        if (!ret) ret = wimlib_reference_resources(wim, &source, 1, 0);
        printf("reference:%d\n", ret);
        if (ret) { wimlib_free(wim); wimlib_free(source); return 0; }
    } else wim = source;
    struct state s = {0, 0};
    ret = wimlib_iterate_lookup_table(wim, 0, row, &s);
    printf("full:%d:%u\n", ret, s.count);
    unsigned total = s.count;
    for (unsigned stop = 1; stop <= total; stop++) {
        s.count = 0; s.stop = stop;
        ret = wimlib_iterate_lookup_table(wim, 0, row, &s);
        printf("stop:%u:%d:%u\n", stop, ret, s.count);
    }
    if (wim != source) wimlib_free(wim);
    wimlib_free(source);
    return 0;
}
