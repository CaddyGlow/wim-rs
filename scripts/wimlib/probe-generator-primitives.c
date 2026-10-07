/* Source-only oracle: include the unchanged optional original test backend.
 * Unused generator/comparator sections are discarded by the linker. */
#include "/tmp/wimlib/src/test_support.c"
#include <stdio.h>
int main(int argc, char **argv) {
    struct generation_context ctx = {0};
    unsigned char data[8192];
    utf16lechar name[256];
    if (argc != 4) return 2;
    wimlib_seed_random(strtoull(argv[1], NULL, 0));
    const char *kind = argv[2];
    size_t size = strtoull(argv[3], NULL, 0);
    if (!strcmp(kind, "filename")) {
        size = generate_random_filename(name, size, &ctx) * 2;
        memcpy(data, name, size);
    } else if (!strcmp(kind, "short")) {
        size = generate_random_short_name(name, &ctx) * 2;
        memcpy(data, name, size);
    } else if (!strcmp(kind, "security")) {
        size = generate_random_security_descriptor(data, &ctx);
    } else if (!strcmp(kind, "data")) {
        if (size > sizeof(data)) return 3;
        generate_data(data, size, &ctx);
    } else if (!strcmp(kind, "timestamp")) {
        u64 timestamp = cpu_to_le64(generate_random_timestamp());
        memcpy(data, &timestamp, 8);
        size = 8;
    } else return 4;
    for (size_t i = 0; i < size; i++) printf("%02x", data[i]);
    printf("\nnext=%08x\n", rand32());
    return 0;
}
