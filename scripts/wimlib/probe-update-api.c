#include <wimlib.h>
#include <errno.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stddef.h>
static WIMStruct *active;
static int calls, abort_at, status, change;
static int entry(const struct wimlib_dir_entry *d, void *context);
static enum wimlib_progress_status progress(enum wimlib_progress_msg event,
        union wimlib_progress_info *info, void *context)
{
    (void)context;
    const struct wimlib_update_command *c = info->update.command;
    printf("event:%d:%zu:%zu:%d:", event, info->update.completed_commands,
           info->update.total_commands, c->op);
    if (c->op == WIMLIB_UPDATE_OP_DELETE)
        printf("%s:%d\n", c->delete_.wim_path, c->delete_.delete_flags);
    else printf("%s:%s:%d\n", c->rename.wim_source_path,
                c->rename.wim_target_path, c->rename.rename_flags);
    if (change == 1) wimlib_register_progress_function(active, NULL, NULL);
    if (change == 2)
        printf("callback-tree:%d\n", wimlib_iterate_dir_tree(active,1,"/",WIMLIB_ITERATE_DIR_TREE_FLAG_RECURSIVE,entry,NULL));
    calls++;
    return calls == abort_at ? status : WIMLIB_PROGRESS_STATUS_CONTINUE;
}
static int entry(const struct wimlib_dir_entry *d, void *context)
{
    (void)context;
    printf("entry:%s:%" PRIu32 ":%" PRIu64 "\n", d->full_path,
           d->attributes, d->streams[0].resource.uncompressed_size);
    return 0;
}
int main(int argc, char **argv)
{
    if (argc != 8) return 2;
    printf("layout:%zu:%zu:%zu:%zu:%zu:%zu\n",sizeof(struct wimlib_update_command),
        offsetof(struct wimlib_update_command,delete_),sizeof(struct wimlib_add_command),
        sizeof(struct wimlib_delete_command),sizeof(struct wimlib_rename_command),
        sizeof(((union wimlib_progress_info *)0)->update));
    int r = wimlib_open_wim(argv[1], 0, &active);
    if (r) { printf("open:%d\n",r); return 0; }
    abort_at=atoi(argv[4]); status=atoi(argv[5]); change=atoi(argv[7]);
    wimlib_register_progress_function(active, progress, NULL);
    struct wimlib_update_command c[2]; memset(c,0,sizeof(c));
    c[0].op=WIMLIB_UPDATE_OP_RENAME;
    c[0].rename.wim_source_path="file"; c[0].rename.wim_target_path="new";
    c[1].op=WIMLIB_UPDATE_OP_DELETE; c[1].delete_.wim_path="missing";
    size_t count=2;
    switch(atoi(argv[2])) {
    case 0: count=0; break;
    case 2: c[1].delete_.wim_path="new"; break;
    case 3: c[0].op=WIMLIB_UPDATE_OP_DELETE; c[0].delete_.wim_path="dir";
            c[0].delete_.delete_flags=WIMLIB_DELETE_FLAG_RECURSIVE;
            c[1].op=WIMLIB_UPDATE_OP_RENAME; c[1].rename.wim_source_path="alias";
            c[1].rename.wim_target_path="dir/child"; break;
    case 4: c[1].op=WIMLIB_UPDATE_OP_RENAME; c[1].rename.wim_source_path="alias";
            c[1].rename.wim_target_path="file"; break;
    case 5: c[0].op=WIMLIB_UPDATE_OP_DELETE; c[0].delete_.wim_path="missing";
            c[0].delete_.delete_flags=WIMLIB_DELETE_FLAG_FORCE;
            c[1].op=WIMLIB_UPDATE_OP_RENAME; c[1].rename.wim_source_path="file";
            c[1].rename.wim_target_path="new"; break;
    case 6: c[1].op=WIMLIB_UPDATE_OP_RENAME; c[1].rename.wim_source_path="alias";
            c[1].rename.wim_target_path="other"; c[1].rename.rename_flags=1; break;
    case 7: c[1].op=3; break;
    case 8: c[0].op=WIMLIB_UPDATE_OP_DELETE; c[0].delete_.wim_path="/";
            c[0].delete_.delete_flags=WIMLIB_DELETE_FLAG_RECURSIVE;
            c[1].op=WIMLIB_UPDATE_OP_RENAME; c[1].rename.wim_source_path="file";
            c[1].rename.wim_target_path="new"; break;
    case 9: c[0].rename.wim_source_path="\\file\\";
            c[0].rename.wim_target_path="dir//child/"; count=1; break;
    }
    errno=0;
    r=wimlib_update_image(active,atoi(argv[6]),count?c:NULL,count,atoi(argv[3]));
    printf("result:%d:errno:%d\n",r,errno);
    printf("tree:%d\n",wimlib_iterate_dir_tree(active,1,"/",WIMLIB_ITERATE_DIR_TREE_FLAG_RECURSIVE,entry,NULL));
    wimlib_free(active);
    return 0;
}
