/* Host ABI oracle: compile against the unchanged upstream public header. */
#include <stddef.h>
#include <stdio.h>
#include <wimlib.h>

int main(void)
{
    printf("{\n");
#define SIZE(name, type) printf("  \"" name "\": %zu,\n", sizeof(type))
    SIZE("pointer_size", void *);
    SIZE("size_t_size", size_t);
    SIZE("tchar_size", wimlib_tchar);
    SIZE("timespec_size", struct wimlib_timespec);
    SIZE("wim_info_size", struct wimlib_wim_info);
    SIZE("resource_entry_size", struct wimlib_resource_entry);
    SIZE("dir_entry_size", struct wimlib_dir_entry);
    SIZE("stream_entry_size", struct wimlib_stream_entry);
    SIZE("progress_info_size", union wimlib_progress_info);
    SIZE("capture_source_size", struct wimlib_capture_source);
    SIZE("update_command_size", struct wimlib_update_command);
    SIZE("compression_enum_size", enum wimlib_compression_type);
    SIZE("error_enum_size", enum wimlib_error_code);
    printf("  \"version\": %u\n}\n",
           (WIMLIB_MAJOR_VERSION << 20) | (WIMLIB_MINOR_VERSION << 10) |
               WIMLIB_PATCH_VERSION);
    return 0;
}
