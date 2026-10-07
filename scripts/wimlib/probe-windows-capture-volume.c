/* Whole-directory capture and independent verification using the original header. */
#include <windows.h>
#include <wimlib.h>
#include <stdio.h>
#include <wchar.h>

typedef int (__cdecl *init_fn)(int);
typedef int (__cdecl *create_fn)(int, WIMStruct **);
typedef int (__cdecl *add_fn)(WIMStruct *, const wchar_t *, const wchar_t *, const wchar_t *, int);
typedef int (__cdecl *write_fn)(WIMStruct *, const wchar_t *, int, int, unsigned);
typedef int (__cdecl *open_fn)(const wchar_t *, int, WIMStruct **);
typedef int (__cdecl *verify_fn)(WIMStruct *, int);
typedef int (__cdecl *tree_fn)(WIMStruct *, int, const wchar_t *, int, wimlib_iterate_dir_tree_callback_t, void *);
typedef void (__cdecl *free_fn)(WIMStruct *);
struct totals { unsigned long long entries, directories, reparses, streams, bytes; };
static int count(const struct wimlib_dir_entry *d, void *context) {
    struct totals *t = context;
    t->entries++;
    t->directories += !!(d->attributes & FILE_ATTRIBUTE_DIRECTORY);
    t->reparses += !!(d->attributes & FILE_ATTRIBUTE_REPARSE_POINT);
    t->streams += d->num_named_streams;
    for (unsigned i = 0; i <= d->num_named_streams; i++)
        t->bytes += d->streams[i].resource.uncompressed_size;
    return 0;
}
int wmain(int argc, wchar_t **argv) {
    if (argc != 5) return 2;
    setvbuf(stdout, NULL, _IONBF, 0);
    HMODULE writer = LoadLibraryW(argv[1]), reader = LoadLibraryW(argv[4]);
    if (!writer || !reader) { printf("loader %lu\n", GetLastError()); return 3; }
    init_fn init = (init_fn)GetProcAddress(writer, "wimlib_global_init");
    create_fn create = (create_fn)GetProcAddress(writer, "wimlib_create_new_wim");
    add_fn add = (add_fn)GetProcAddress(writer, "wimlib_add_image");
    write_fn write = (write_fn)GetProcAddress(writer, "wimlib_write");
    free_fn release = (free_fn)GetProcAddress(writer, "wimlib_free");
    open_fn open = (open_fn)GetProcAddress(reader, "wimlib_open_wim");
    verify_fn verify = (verify_fn)GetProcAddress(reader, "wimlib_verify_wim");
    tree_fn tree = (tree_fn)GetProcAddress(reader, "wimlib_iterate_dir_tree");
    free_fn reader_release = (free_fn)GetProcAddress(reader, "wimlib_free");
    if (!init || !create || !add || !write || !release || !open || !verify || !tree || !reader_release) return 77;
    WIMStruct *w = NULL;
    int rc = init(4 | 8); printf("init %d\n", rc); if (rc) return 4;
    rc = create(WIMLIB_COMPRESSION_TYPE_NONE, &w); printf("create %d\n", rc); if (rc) return 5;
    rc = add(w, argv[2], L"Windows metadata validation", NULL, WIMLIB_ADD_FLAG_STRICT_ACLS);
    printf("add %d\n", rc);
    if (!rc) { rc = write(w, argv[3], 1, WIMLIB_WRITE_FLAG_CHECK_INTEGRITY, 1); printf("write %d\n", rc); }
    release(w); if (rc) return 6;
    rc = open(argv[3], WIMLIB_OPEN_FLAG_CHECK_INTEGRITY, &w); printf("independent-open %d\n", rc); if (rc) return 7;
    rc = verify(w, 0); printf("independent-verify %d\n", rc);
    if (!rc) {
        struct totals t = {0};
        rc = tree(w, 1, NULL, WIMLIB_ITERATE_DIR_TREE_FLAG_RECURSIVE, count, &t);
        printf("independent-tree %d entries=%llu directories=%llu reparses=%llu streams=%llu bytes=%llu\n",
               rc, t.entries, t.directories, t.reparses, t.streams, t.bytes);
    }
    reader_release(w); FreeLibrary(reader); FreeLibrary(writer);
    return rc ? 8 : 0;
}
