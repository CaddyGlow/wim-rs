/* Unchanged-header image XML/property ABI differential client. */
#include <stdio.h>
#include <wimlib.h>
static void value(const char *label, const char *s) {
    printf("%s=", label);
    if (!s) printf("NULL");
    else for (; *s; s++) printf("%02x", (unsigned char)*s);
    putchar('\n');
}
int main(int argc, char **argv) {
    WIMStruct *w = NULL;
    if (argc != 2 && argc != 3) return 2;
    int ret = wimlib_open_wim(argv[1], 0, &w);
    printf("open=%d\n", ret);
    if (ret) return 3;
    if (argc == 3) {
        printf("invalid_utf8_set=%d\n", wimlib_set_image_name(w, 1, "\xff"));
        value("invalid_utf8_name", wimlib_get_image_name(w, 1));
        printf("invalid_utf8_resolve=%d\n", wimlib_resolve_image(w, "\xff"));
        printf("invalid_utf8_in_use=%d\n", wimlib_image_name_in_use(w, "\xff"));
        printf("invalid_utf8_collision=%d\n", wimlib_set_image_name(w, 2, "\xff"));
        printf("invalid_utf8_write=%d\n", wimlib_write(w, argv[2], WIMLIB_ALL_IMAGES, 0, 1));
        printf("invalid_utf8_clear=%d\n", wimlib_set_image_name(w, 1, NULL));
        printf("invalid_path_set=%d\n", wimlib_set_image_property(w, 1, "\xfe/NODE", "\xfd\xfc<&"));
        value("invalid_path_get", wimlib_get_image_property(w, 1, "\xfe/NODE"));
        printf("invalid_path_write=%d\n", wimlib_write(w, argv[2], WIMLIB_ALL_IMAGES, 0, 1));
        printf("invalid_path_delete_child=%d\n", wimlib_set_image_property(w, 1, "\xfe/NODE", NULL));
        printf("invalid_path_empty_ancestor_write=%d\n", wimlib_write(w, argv[2], WIMLIB_ALL_IMAGES, 0, 1));
        printf("invalid_path_delete_ancestor=%d\n", wimlib_set_image_property(w, 1, "\xfe", NULL));
        printf("corrected_write=%d\n", wimlib_write(w, argv[2], WIMLIB_ALL_IMAGES, 0, 1));
        const char *wtf8[] = {"\xed\xa0\x80", "\xed\xaf\xbf", "\xed\xb0\x80", "\xed\xbf\xbf", "\xed\xa0\x80\xed\xb0\x80", "\xf0\x90\x80\x80", "\xef\xbf\xbe", "\xef\xbf\xbf"};
        for (unsigned i=0;i<sizeof(wtf8)/sizeof(*wtf8);i++) {
            printf("wtf8_set=%d\n",wimlib_set_image_name(w,1,wtf8[i]));
            value("wtf8_get",wimlib_get_image_name(w,1));
            printf("wtf8_write=%d\n",wimlib_write(w,argv[2],WIMLIB_ALL_IMAGES,0,1));
            WIMStruct *reopened=NULL;
            int reopened_result=wimlib_open_wim(argv[2],0,&reopened);
            printf("wtf8_reopen=%d\n",reopened_result);
            if (!reopened_result) {value("wtf8_reopened_name",wimlib_get_image_name(reopened,1));wimlib_free(reopened);}
        }
        wimlib_free(w);
        return 0;
    }
    for (int i = -1; i <= 3; i++) {
        value("name", wimlib_get_image_name(w, i));
        value("description", wimlib_get_image_description(w, i));
        value("missing", wimlib_get_image_property(w, i, "UNKNOWN"));
    }
    const char *selectors[] = {NULL,"","all","ALL","*","1","2","3","0","+1"," 2","1 ","-1","184467440737095516160","First","first"};
    for (unsigned i = 0; i < sizeof(selectors)/sizeof(*selectors); i++) printf("resolve=%d\n", wimlib_resolve_image(w, selectors[i]));
    const char *names[] = {NULL,"","First","first","Second"};
    for (unsigned i = 0; i < sizeof(names)/sizeof(*names); i++) printf("in_use=%d\n", wimlib_image_name_in_use(w, names[i]));
    const char *saved = wimlib_get_image_name(w, 1);
    printf("collision=%d\n", wimlib_set_image_name(w, 1, "Second"));
    value("saved_after_collision", saved);
    printf("rename=%d\n", wimlib_set_image_name(w, 1, "Renamed"));
    value("renamed", wimlib_get_image_name(w, 1));
    printf("description=%d\n", wimlib_set_image_descripton(w, 1, "d\xc3\xa9scription & <value>"));
    value("description", wimlib_get_image_description(w, 1));
    printf("flags=%d\n", wimlib_set_image_flags(w, 2, "Professional"));
    value("flags", wimlib_get_image_property(w, 2, "FLAGS"));
    const char *paths[] = {NULL,"","/","BAD SPACE","ROOT/A","ROOT/A[2]","ROOT/A[4]","ROOT/B[0]","ROOT//C","NAME[1]","UNKNOWN"};
    for (unsigned i = 0; i < sizeof(paths)/sizeof(*paths); i++) {
        printf("set=%d\n", wimlib_set_image_property(w, 1, paths[i], "text"));
        value("get", wimlib_get_image_property(w, 1, paths[i]));
        printf("invalid_image=%d\n", wimlib_set_image_property(w, 99, paths[i], "text"));
        printf("remove=%d\n", wimlib_set_image_property(w, 1, paths[i], NULL));
    }
    printf("bad_value=%d\n", wimlib_set_image_property(w, 99, "NAME", "illegal\001"));
    printf("clear_name=%d\n", wimlib_set_image_name(w, 1, NULL));
    value("empty_name", wimlib_get_image_name(w, 1));
    printf("clear_description=%d\n", wimlib_set_image_descripton(w, 1, ""));
    value("empty_description", wimlib_get_image_description(w, 1));
    for (unsigned byte = 1; byte <= 255; byte++) {
        const char input[] = {(char)byte, 0};
        printf("byte_%u_set=%d\n", byte, wimlib_set_image_name(w, 1, input));
        value("byte_name", wimlib_get_image_name(w, 1));
        printf("byte_resolve=%d\n", wimlib_resolve_image(w, input));
        printf("byte_in_use=%d\n", wimlib_image_name_in_use(w, input));
    }
    for (unsigned byte = 128; byte <= 255; byte++) {
        const char path[] = {(char)byte, '/', 'N', 0};
        const char ancestor[] = {(char)byte, 0};
        printf("path_%u_set=%d\n", byte, wimlib_set_image_property(w, 1, path, "value"));
        value("byte_path", wimlib_get_image_property(w, 1, path));
        printf("byte_path_delete=%d\n", wimlib_set_image_property(w, 1, ancestor, NULL));
    }
    wimlib_free(w);
    return 0;
}
