# wimlib 1.14.5 public API reference

This reference describes the local upstream source at `/tmp/wimlib`, commit
`cd5e231c348c255ae5088873b5a66ee0eb96fa07`. Its compatibility authority is
[`include/wimlib.h`](/tmp/wimlib/include/wimlib.h), not a wrapper's currently used
subset. Every exported declaration in that header is included below. The
function contracts are transcribed from upstream's API comments with light
Markdown conversion; declarations, constants and public layout definitions are
preserved literally. Upstream wording describes supported behavior, not evidence
that a future Rust implementation has passed its gates.

## Operational model and ownership

`WIMStruct` is an opaque, reference-managed image container. Open or create it,
perform logical edits, then explicitly write or overwrite to persist changes.
Images are numbered from 1; `WIMLIB_NO_IMAGE` is 0 and `WIMLIB_ALL_IMAGES` is -1,
and each function explicitly determines whether the latter is accepted. Deleting
an image renumbers later images. Capture initially scans metadata and can defer
reading payloads until write: source data must remain accessible and unchanged.
Export and resource referencing retain dependencies on other WIM handles. Calling
`wimlib_free()` releases the caller's reference; shared internal references delay
actual deallocation and file closure. Null free is supported. After successful
`wimlib_overwrite()`, only freeing that handle is allowed; reopen to keep working.
`WIMLIB_EXPORT_FLAG_GIFT` also promises that the source handle will no longer
be accessed after export except for freeing it. Diagnostic observations from
using a gifted source again do not establish a valid-lifecycle compatibility gate.

Image property getters return borrowed library-owned strings; do not free them
or assume they survive later library calls; duplicate them when retaining them. Compression/error/version string getters
return library-owned values. `wimlib_get_xml_data()` allocates raw UTF-16LE bytes
and returns a byte count, not a host text string. `wimlib_load_text_file()` returns
an allocated host `wimlib_tchar` string and a character count; its header explicitly
requires `free()`. Allocator customization is process-wide and must remain
consistent across allocations and deallocation. The XML implementation frees
its allocated buffer with upstream `FREE()`; see
[`src/xml.c:1094`](/tmp/wimlib/src/xml.c:1094) and
[`src/textfile.c:388`](/tmp/wimlib/src/textfile.c:388). Callback data and its pointer
fields are borrowed for the callback duration; copy retained observations.

A single `WIMStruct` cannot be accessed concurrently, including read operations.
Different handles may run concurrently; never overwrite a file while another
handle uses that same on-disk file. Initialization, cleanup, allocator and error
configuration functions mutate global state and require only one active library
thread. Automatic initialization exists, but explicit initialization allows flags
and privilege policy to be controlled before operations. Cleanup must follow
release of live handles and completed operations.

## ABI and Rust replacement requirements

The header's `WIMLIBAPI` also controls export visibility. On 32-bit x86 GNU
builds, `WIMLIB_ALIGN_STACK` uses `force_align_arg_pointer` to realign the incoming
stack. Audit this caller compatibility requirement when designing the Rust
export boundary; matching symbol names and `extern "C"` alone does not prove it.

A source-compatible Rust API alone is insufficient for a drop-in C library.
Preserve C symbol spellings and the documented `cdecl` calling convention.
Unix callers link with `-lwim`; Windows consumers expect `libwim-15.dll` and
compatible import libraries (`libwim.lib`). Preserve the historical
`wimlib_set_image_descripton` misspelling. Supply the same exported surface and
compatible headers, C integer widths, bool layout, native pointer/size_t widths,
opaque handle representations, tagged unions, flexible-array trailing entries,
reserved fields and exact numeric enum/mask values. The annotated definitions
below are the layout authority; reproduce each supported target ABI with C
layout probes rather than assuming one Linux layout applies to Windows.

`wimlib_tchar` is UTF-16LE `wchar_t` on Windows and UTF-8 `char` on Unix. Surrogate
codepoints are accepted for Windows filename interoperability; Rust `String`
alone cannot represent the whole contract. Windows timestamps deliberately use
32-bit seconds on 32-bit Windows and 64-bit seconds on 64-bit Windows, with
additional high timestamp fields in directory entries. On Unix the timestamp
layout is the target's `struct timespec`. `FILE*` functions require interoperability
with the caller's C runtime; descriptor APIs use borrowed descriptors and do not
transfer ownership unless their individual contracts say otherwise. Panic must
never unwind across the C boundary. Allocation failures and callback cancellation
must become the documented numeric return codes, with no process abort replacing
recoverable upstream failures.

Most functions return 0 or a `wimlib_error_code`, but exceptions matter: string
getters return pointers, resolution returns an image index/sentinel, boolean
queries return bool, memory estimation returns bytes or 0, compression returns
compressed size or 0, and low-level decompression returns 0 or -1. Do not funnel
these through one generic status adapter. Error values are sparse and cannot be
renumbered. Diagnostic printing is optional and separate from error returns.

Progress callbacks use a message discriminator plus `union wimlib_progress_info`.
Return CONTINUE or ABORT, and preserve abort/unknown-status error distinctions.
Only explicitly documented payload fields are writable (for example exclusion
and error-policy responses). Directory and lookup-table iteration callbacks
return 0 to continue; a nonzero value stops traversal and propagates to the
caller. Callback ordering, begin/end events, totals, mutation responses and
cancellation points are observable API behavior and require differential tests.

## Public functions by subsystem

The per-function sections preserve parameter conditions, flags, unsupported
cases, return values and explicitly documented error paths. Flags are bit masks;
unspecified flag parameters reserved for future use must remain zero. All
flag constants and their full annotated semantics appear after these sections.

## Creation, opening, and lifecycle

### `wimlib_create_new_wim`

Source: [include/wimlib.h:2786](/tmp/wimlib/include/wimlib.h:2786).

```c
int
wimlib_create_new_wim(enum wimlib_compression_type ctype, WIMStruct **wim_ret);
```

Create a `WIMStruct` which initially contains no images and is not backed by
an on-disk file.

**Parameter `ctype`:**
	The "output compression type" to assign to the `WIMStruct`.  This is the
	compression type that will be used if the `WIMStruct` is later persisted
	to an on-disk file using wimlib_write().
	<br/>
	This choice is not necessarily final.  If desired, it can still be
	changed at any time before wimlib_write() is called, using
	wimlib_set_output_compression_type().  In addition, if you wish to use a
	non-default compression chunk size, then you will need to call
	wimlib_set_output_chunk_size().
**Parameter `wim_ret`:**
	On success, a pointer to the new `WIMStruct` is written to the memory
	location pointed to by this parameter.  This `WIMStruct` must be freed
	using wimlib_free() when finished with it.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_INVALID_COMPRESSION_TYPE`:**
	`ctype` was not a supported compression type.
**Error `WIMLIB_ERR_NOMEM`:**
	Insufficient memory to allocate a new `WIMStruct`.

### `wimlib_free`

Source: [include/wimlib.h:3214](/tmp/wimlib/include/wimlib.h:3214).

```c
void
wimlib_free(WIMStruct *wim);
```

Release a reference to a `WIMStruct`.  If the `WIMStruct` is still referenced
by other `WIMStruct`'s (e.g. following calls to wimlib_export_image() or
wimlib_reference_resources()), then the library will free it later, when the
last reference is released; otherwise it is freed immediately and any
associated file descriptors are closed.

**Parameter `wim`:**
	Pointer to the `WIMStruct` to release.  If `NULL,` no action is taken.

### `wimlib_global_init`

Source: [include/wimlib.h:3390](/tmp/wimlib/include/wimlib.h:3390).

```c
int
wimlib_global_init(int init_flags);
```

Initialization function for wimlib.  Call before using any other wimlib
function (except possibly wimlib_set_print_errors()).  If not done manually,
this function will be called automatically with a flags argument of 0.  This
function does nothing if called again after it has already successfully run.

**Parameter `init_flags`:**
	Bitwise OR of flags prefixed with WIMLIB_INIT_FLAG.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_INSUFFICIENT_PRIVILEGES`:**
	`WIMLIB_INIT_FLAG_STRICT_APPLY_PRIVILEGES` and/or
	`WIMLIB_INIT_FLAG_STRICT_CAPTURE_PRIVILEGES` were specified in `init_flags,` but the corresponding privileges could not be acquired.

### `wimlib_global_cleanup`

Source: [include/wimlib.h:3399](/tmp/wimlib/include/wimlib.h:3399).

```c
void
wimlib_global_cleanup(void);
```

Cleanup function for wimlib.  You are not required to call this function, but
it will release any global resources allocated by the library.

### `wimlib_open_wim`

Source: [include/wimlib.h:3737](/tmp/wimlib/include/wimlib.h:3737).

```c
int
wimlib_open_wim(const wimlib_tchar *wim_file,
		int open_flags,
		WIMStruct **wim_ret);
```

Open a WIM file and create a `WIMStruct` for it.

**Parameter `wim_file`:**
	The path to the WIM file to open.
**Parameter `open_flags`:**
	Bitwise OR of flags prefixed with WIMLIB_OPEN_FLAG.
**Parameter `wim_ret`:**
	On success, a pointer to a new `WIMStruct` backed by the specified
	on-disk WIM file is written to the memory location pointed to by this
	parameter.  This `WIMStruct` must be freed using wimlib_free() when
	finished with it.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_IMAGE_COUNT`:**
	The number of metadata resources found in the WIM did not match the
	image count specified in the WIM header, or the number of &lt;IMAGE&gt;
	elements in the XML data of the WIM did not match the image count
	specified in the WIM header.
**Error `WIMLIB_ERR_INTEGRITY`:**
	`WIMLIB_OPEN_FLAG_CHECK_INTEGRITY` was specified in `open_flags,` and
	the WIM file failed the integrity check.
**Error `WIMLIB_ERR_INVALID_CHUNK_SIZE`:**
	The library did not recognize the compression chunk size of the WIM as
	valid for its compression type.
**Error `WIMLIB_ERR_INVALID_COMPRESSION_TYPE`:**
	The library did not recognize the compression type of the WIM.
**Error `WIMLIB_ERR_INVALID_HEADER`:**
	The header of the WIM was otherwise invalid.
**Error `WIMLIB_ERR_INVALID_INTEGRITY_TABLE`:**
	`WIMLIB_OPEN_FLAG_CHECK_INTEGRITY` was specified in `open_flags` and
	the WIM contained an integrity table, but the integrity table was
	invalid.
**Error `WIMLIB_ERR_INVALID_LOOKUP_TABLE_ENTRY`:**
	The lookup table of the WIM was invalid.
**Error `WIMLIB_ERR_INVALID_PARAM`:**
	`wim_ret` was `NULL;` or, `wim_file` was not a nonempty string.
**Error `WIMLIB_ERR_IS_SPLIT_WIM`:**
	The WIM was a split WIM and `WIMLIB_OPEN_FLAG_ERROR_IF_SPLIT` was
	specified in `open_flags.`
**Error `WIMLIB_ERR_NOT_A_WIM_FILE`:**
	The file did not begin with the magic characters that identify a WIM
	file.
**Error `WIMLIB_ERR_OPEN`:**
	Failed to open the WIM file for reading.  Some possible reasons: the WIM
	file does not exist, or the calling process does not have permission to
	open it.
**Error `WIMLIB_ERR_READ`:**
	Failed to read data from the WIM file.
**Error `WIMLIB_ERR_UNEXPECTED_END_OF_FILE`:**
	Unexpected end-of-file while reading data from the WIM file.
**Error `WIMLIB_ERR_UNKNOWN_VERSION`:**
	The WIM version number was not recognized. (May be a pre-Vista WIM.)
**Error `WIMLIB_ERR_WIM_IS_ENCRYPTED`:**
	The WIM cannot be opened because it contains encrypted segments.  (It
	may be a Windows 8 "ESD" file.)
**Error `WIMLIB_ERR_WIM_IS_INCOMPLETE`:**
	The WIM file is not complete (e.g. the program which wrote it was
	terminated before it finished)
**Error `WIMLIB_ERR_WIM_IS_READONLY`:**
	`WIMLIB_OPEN_FLAG_WRITE_ACCESS` was specified but the WIM file was
	considered read-only because of any of the reasons mentioned in the
	documentation for the `WIMLIB_OPEN_FLAG_WRITE_ACCESS` flag.
**Error `WIMLIB_ERR_XML`:**
	The XML data of the WIM was invalid.

### `wimlib_open_wim_with_progress`

Source: [include/wimlib.h:3753](/tmp/wimlib/include/wimlib.h:3753).

```c
int
wimlib_open_wim_with_progress(const wimlib_tchar *wim_file,
			      int open_flags,
			      WIMStruct **wim_ret,
			      wimlib_progress_func_t progfunc,
			      void *progctx);
```

Same as wimlib_open_wim(), but allows specifying a progress function and
progress context.  If successful, the progress function will be registered in
the newly open `WIMStruct`, as if by an automatic call to
wimlib_register_progress_function().  In addition, if
`WIMLIB_OPEN_FLAG_CHECK_INTEGRITY` is specified in `open_flags,` then the
progress function will receive `WIMLIB_PROGRESS_MSG_VERIFY_INTEGRITY`
messages while checking the WIM file's integrity.

## Capture and image mutation

### `wimlib_add_empty_image`

Source: [include/wimlib.h:2663](/tmp/wimlib/include/wimlib.h:2663).

```c
int
wimlib_add_empty_image(WIMStruct *wim,
		       const wimlib_tchar *name,
		       int *new_idx_ret);
```

Append an empty image to a `WIMStruct`.

The new image will initially contain no files or directories, although if
written without further modifications, then a root directory will be created
automatically for it.

After calling this function, you can use wimlib_update_image() to add files
to the new image.  This gives you more control over making the new image
compared to calling wimlib_add_image() or wimlib_add_image_multisource().

**Parameter `wim`:**
	Pointer to the `WIMStruct` to which to add the image.
**Parameter `name`:**
	Name to give the new image.  If `NULL` or empty, the new image is given
	no name.  If nonempty, it must specify a name that does not already
	exist in `wim.`
**Parameter `new_idx_ret`:**
	If non-`NULL`, the index of the newly added image is returned in
	this location.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_IMAGE_NAME_COLLISION`:**
	The WIM already contains an image with the requested name.

### `wimlib_add_image`

Source: [include/wimlib.h:2719](/tmp/wimlib/include/wimlib.h:2719).

```c
int
wimlib_add_image(WIMStruct *wim,
		 const wimlib_tchar *source,
		 const wimlib_tchar *name,
		 const wimlib_tchar *config_file,
		 int add_flags);
```

Add an image to a `WIMStruct` from an on-disk directory tree or NTFS volume.

The directory tree or NTFS volume is scanned immediately to load the dentry
tree into memory, and file metadata is read.  However, actual file data may
not be read until the `WIMStruct` is persisted to disk using wimlib_write()
or wimlib_overwrite().

See the documentation for the `wimlib-imagex` program for more information
about the "normal" capture mode versus the NTFS capture mode (entered by
providing the flag `WIMLIB_ADD_FLAG_NTFS`).

Note that no changes are committed to disk until wimlib_write() or
wimlib_overwrite() is called.

**Parameter `wim`:**
	Pointer to the `WIMStruct` to which to add the image.
**Parameter `source`:**
	A path to a directory or unmounted NTFS volume that will be captured as
	a WIM image.
**Parameter `name`:**
	Name to give the new image.  If `NULL` or empty, the new image is given
	no name.  If nonempty, it must specify a name that does not already
	exist in `wim.`
**Parameter `config_file`:**
	Path to capture configuration file, or `NULL.`  This file may specify,
	among other things, which files to exclude from capture.  See the
	documentation for wimcapture (--config option) for details
	of the file format.  If `NULL,` the default capture configuration will
	be used.  Ordinarily, the default capture configuration will result in
	no files being excluded from capture purely based on name; however, the
	`WIMLIB_ADD_FLAG_WINCONFIG` and `WIMLIB_ADD_FLAG_WIMBOOT` flags modify
	the default.
**Parameter `add_flags`:**
	Bitwise OR of flags prefixed with WIMLIB_ADD_FLAG.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

This function is implemented by calling wimlib_add_empty_image(), then
calling wimlib_update_image() with a single "add" command, so any error code
returned by wimlib_add_empty_image() may be returned, as well as any error
codes returned by wimlib_update_image() other than ones documented as only
being returned specifically by an update involving delete or rename commands.

If a progress function is registered with `wim,` then it will receive the
messages `WIMLIB_PROGRESS_MSG_SCAN_BEGIN` and `WIMLIB_PROGRESS_MSG_SCAN_END`.
In addition, if `WIMLIB_ADD_FLAG_VERBOSE` is specified in `add_flags,` it
will receive `WIMLIB_PROGRESS_MSG_SCAN_DENTRY`.

### `wimlib_add_image_multisource`

Source: [include/wimlib.h:2736](/tmp/wimlib/include/wimlib.h:2736).

```c
int
wimlib_add_image_multisource(WIMStruct *wim,
			     const struct wimlib_capture_source *sources,
			     size_t num_sources,
			     const wimlib_tchar *name,
			     const wimlib_tchar *config_file,
			     int add_flags);
```

This function is equivalent to wimlib_add_image() except it allows for
multiple sources to be combined into a single WIM image.  This is done by
specifying the `sources` and `num_sources` parameters instead of the `source` parameter of wimlib_add_image().  The rest of the parameters are the
same as wimlib_add_image().  See the documentation for wimcapture for
full details on how this mode works.

### `wimlib_add_tree`

Source: [include/wimlib.h:2753](/tmp/wimlib/include/wimlib.h:2753).

```c
int
wimlib_add_tree(WIMStruct *wim, int image,
		const wimlib_tchar *fs_source_path,
		const wimlib_tchar *wim_target_path, int add_flags);
```

Add the file or directory tree at `fs_source_path` on the filesystem to the
location `wim_target_path` within the specified `image` of the `wim.`

This just builds an appropriate `wimlib_add_command` and passes it to
wimlib_update_image().

### `wimlib_delete_image`

Source: [include/wimlib.h:2817](/tmp/wimlib/include/wimlib.h:2817).

```c
int
wimlib_delete_image(WIMStruct *wim, int image);
```

Delete an image, or all images, from a `WIMStruct`.

Note that no changes are committed to disk until wimlib_write() or
wimlib_overwrite() is called.

**Parameter `wim`:**
	Pointer to the `WIMStruct` from which to delete the image.
**Parameter `image`:**
	The 1-based index of the image to delete, or `WIMLIB_ALL_IMAGES` to
	delete all images.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_INVALID_IMAGE`:**
	`image` does not exist in the WIM.

This function can additionally return `WIMLIB_ERR_DECOMPRESSION`,
`WIMLIB_ERR_INVALID_METADATA_RESOURCE`, `WIMLIB_ERR_METADATA_NOT_FOUND`,
`WIMLIB_ERR_READ`, or `WIMLIB_ERR_UNEXPECTED_END_OF_FILE`, all of which
indicate failure (for different reasons) to read the metadata resource for an
image that needed to be deleted.

If this function fails when `image` was `WIMLIB_ALL_IMAGES`, then it's
possible that some but not all of the images were deleted.

### `wimlib_delete_path`

Source: [include/wimlib.h:2828](/tmp/wimlib/include/wimlib.h:2828).

```c
int
wimlib_delete_path(WIMStruct *wim, int image,
		   const wimlib_tchar *path, int delete_flags);
```

Delete the `path` from the specified `image` of the `wim.`

This just builds an appropriate `wimlib_delete_command` and passes it to
wimlib_update_image().

### `wimlib_export_image`

Source: [include/wimlib.h:2900](/tmp/wimlib/include/wimlib.h:2900).

```c
int
wimlib_export_image(WIMStruct *src_wim, int src_image,
		    WIMStruct *dest_wim,
		    const wimlib_tchar *dest_name,
		    const wimlib_tchar *dest_description,
		    int export_flags);
```

Export an image, or all images, from a `WIMStruct` into another `WIMStruct`.

Specifically, if the destination `WIMStruct` contains `n` images, then
the source image(s) will be appended, in order, starting at destination index
`n + 1`.  By default, all image metadata will be exported verbatim,
but certain changes can be made by passing appropriate parameters.

wimlib_export_image() is only an in-memory operation; no changes are
committed to disk until wimlib_write() or wimlib_overwrite() is called.

A limitation of the current implementation of wimlib_export_image() is that
the directory tree of a source or destination image cannot be updated
following an export until one of the two images has been freed from memory.

**Parameter `src_wim`:**
	The WIM from which to export the images, specified as a pointer to the
	`WIMStruct` for a standalone WIM file, a delta WIM file, or part 1 of a
	split WIM.  In the case of a WIM file that is not standalone, this
	`WIMStruct` must have had any needed external resources previously
	referenced using wimlib_reference_resources() or
	wimlib_reference_resource_files().
**Parameter `src_image`:**
	The 1-based index of the image from `src_wim` to export, or
	`WIMLIB_ALL_IMAGES`.
**Parameter `dest_wim`:**
	The `WIMStruct` to which to export the images.
**Parameter `dest_name`:**
	For single-image exports, the name to give the exported image in `dest_wim.`  If left `NULL,` the name from `src_wim` is used.  For
	`WIMLIB_ALL_IMAGES` exports, this parameter must be left `NULL;` in
	that case, the names are all taken from `src_wim.`  This parameter is
	overridden by `WIMLIB_EXPORT_FLAG_NO_NAMES`.
**Parameter `dest_description`:**
	For single-image exports, the description to give the exported image in
	the new WIM file.  If left `NULL,` the description from `src_wim` is
	used.  For `WIMLIB_ALL_IMAGES` exports, this parameter must be left `NULL;` in that case, the description are all taken from `src_wim.`  This
	parameter is overridden by `WIMLIB_EXPORT_FLAG_NO_DESCRIPTIONS`.
**Parameter `export_flags`:**
	Bitwise OR of flags prefixed with WIMLIB_EXPORT_FLAG.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_DUPLICATE_EXPORTED_IMAGE`:**
	One or more of the source images had already been exported into the
	destination WIM.
**Error `WIMLIB_ERR_IMAGE_NAME_COLLISION`:**
	One or more of the names being given to an exported image was already in
	use in the destination WIM.
**Error `WIMLIB_ERR_INVALID_IMAGE`:**
	`src_image` does not exist in `src_wim.`
**Error `WIMLIB_ERR_METADATA_NOT_FOUND`:**
	At least one of `src_wim` and `dest_wim` does not contain image
	metadata; for example, one of them represents a non-first part of a
	split WIM.
**Error `WIMLIB_ERR_RESOURCE_NOT_FOUND`:**
	A file data blob that needed to be exported could not be found in the
	blob lookup table of `src_wim.`  See `G_nonstandalone_wims` (upstream header section).

This function can additionally return `WIMLIB_ERR_DECOMPRESSION`,
`WIMLIB_ERR_INVALID_METADATA_RESOURCE`, `WIMLIB_ERR_METADATA_NOT_FOUND`,
`WIMLIB_ERR_READ`, or `WIMLIB_ERR_UNEXPECTED_END_OF_FILE`, all of which
indicate failure (for different reasons) to read the metadata resource for an
image in `src_wim` that needed to be exported.

### `wimlib_rename_path`

Source: [include/wimlib.h:4021](/tmp/wimlib/include/wimlib.h:4021).

```c
int
wimlib_rename_path(WIMStruct *wim, int image,
		   const wimlib_tchar *source_path, const wimlib_tchar *dest_path);
```

Rename the `source_path` to the `dest_path` in the specified `image` of
the `wim.`

This just builds an appropriate `wimlib_rename_command` and passes it to
wimlib_update_image().

### `wimlib_set_image_descripton`

Source: [include/wimlib.h:4101](/tmp/wimlib/include/wimlib.h:4101).

```c
int
wimlib_set_image_descripton(WIMStruct *wim, int image,
			    const wimlib_tchar *description);
```

Change the description of a WIM image.  Equivalent to
`wimlib_set_image_property(wim, image, "DESCRIPTION", description)`.

Note that "description" is misspelled in the name of this function.

### `wimlib_set_image_flags`

Source: [include/wimlib.h:4112](/tmp/wimlib/include/wimlib.h:4112).

```c
int
wimlib_set_image_flags(WIMStruct *wim, int image, const wimlib_tchar *flags);
```

Change what is stored in the \<FLAGS\> element in the WIM XML document
(usually something like "Core" or "Ultimate").  Equivalent to
`wimlib_set_image_property(wim, image, "FLAGS", flags)`.

### `wimlib_set_image_name`

Source: [include/wimlib.h:4121](/tmp/wimlib/include/wimlib.h:4121).

```c
int
wimlib_set_image_name(WIMStruct *wim, int image, const wimlib_tchar *name);
```

Change the name of a WIM image.  Equivalent to
`wimlib_set_image_property(wim, image, "NAME", name)`.

### `wimlib_set_image_property`

Source: [include/wimlib.h:4161](/tmp/wimlib/include/wimlib.h:4161).

```c
int
wimlib_set_image_property(WIMStruct *wim, int image,
			  const wimlib_tchar *property_name,
			  const wimlib_tchar *property_value);
```

Since wimlib v1.8.3: add, modify, or remove a per-image property from the
WIM's XML document.  This is an alternative to wimlib_set_image_name(),
wimlib_set_image_descripton(), and wimlib_set_image_flags() which allows
manipulating any simple string property.

**Parameter `wim`:**
	Pointer to the `WIMStruct` for the WIM.
**Parameter `image`:**
	The 1-based index of the image for which to set the property.
**Parameter `property_name`:**
	The name of the image property in the same format documented for
	wimlib_get_image_property().
	<br/>
	Note: if creating a new element using a bracketed index such as
	"WINDOWS/LANGUAGES/LANGUAGE[2]", the highest index that can be specified
	is one greater than the number of existing elements with that same name,
	excluding the index.  That means that if you are adding a list of new
	elements, they must be added sequentially from the first index (1) to
	the last index (n).
**Parameter `property_value`:**
	If not NULL and not empty, the property is set to this value.
	Otherwise, the property is removed from the XML document.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_IMAGE_NAME_COLLISION`:**
	The user requested to set the image name (the `NAME` property),
	but another image in the WIM already had the requested name.
**Error `WIMLIB_ERR_INVALID_IMAGE`:**
	`image` does not exist in `wim.`
**Error `WIMLIB_ERR_INVALID_PARAM`:**
	`property_name` has an unsupported format, or `property_name` included
	a bracketed index that was too high.

### `wimlib_set_wim_info`

Source: [include/wimlib.h:4314](/tmp/wimlib/include/wimlib.h:4314).

```c
int
wimlib_set_wim_info(WIMStruct *wim, const struct wimlib_wim_info *info,
		    int which);
```

Set basic information about a WIM.

**Parameter `wim`:**
	Pointer to the `WIMStruct` for a WIM.
**Parameter `info`:**
	Pointer to a `wimlib_wim_info` structure that contains the information
	to set.  Only the information explicitly specified in the `which` flags
	need be valid.
**Parameter `which`:**
	Flags that specify which information to set.  This is a bitwise OR of
	`WIMLIB_CHANGE_READONLY_FLAG`, `WIMLIB_CHANGE_GUID`,
	`WIMLIB_CHANGE_BOOT_INDEX`, and/or `WIMLIB_CHANGE_RPFIX_FLAG`.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_IMAGE_COUNT`:**
	`WIMLIB_CHANGE_BOOT_INDEX` was specified, but
	`wimlib_wim_info`.boot_index did not specify 0 or a valid 1-based image
	index in the WIM.

### `wimlib_update_image`

Source: [include/wimlib.h:4544](/tmp/wimlib/include/wimlib.h:4544).

```c
int
wimlib_update_image(WIMStruct *wim,
		    int image,
		    const struct wimlib_update_command *cmds,
		    size_t num_cmds,
		    int update_flags);
```

Update a WIM image by adding, deleting, and/or renaming files or directories.

**Parameter `wim`:**
	Pointer to the `WIMStruct` containing the image to update.
**Parameter `image`:**
	The 1-based index of the image to update.
**Parameter `cmds`:**
	An array of `wimlib_update_command`'s that specify the update operations
	to perform.
**Parameter `num_cmds`:**
	Number of commands in `cmds.`
**Parameter `update_flags`:**
	`WIMLIB_UPDATE_FLAG_SEND_PROGRESS` or 0.

**Return:** 0 on success; a `wimlib_error_code` value on failure.  On failure,
all update commands will be rolled back, and no visible changes will have
been made to `wim.`

**Error `WIMLIB_ERR_FVE_LOCKED_VOLUME`:**
	Windows-only: One of the "add" commands attempted to add files from an
	encrypted BitLocker volume that hasn't yet been unlocked.
**Error `WIMLIB_ERR_IMAGE_HAS_MULTIPLE_REFERENCES`:**
	There are currently multiple references to the image as a result of a
	call to wimlib_export_image().  Free one before attempting the update.
**Error `WIMLIB_ERR_INVALID_CAPTURE_CONFIG`:**
	The contents of a capture configuration file were invalid.
**Error `WIMLIB_ERR_INVALID_IMAGE`:**
	`image` did not exist in `wim.`
**Error `WIMLIB_ERR_INVALID_OVERLAY`:**
	An add command with `WIMLIB_ADD_FLAG_NO_REPLACE` specified attempted to
	replace an existing nondirectory file.
**Error `WIMLIB_ERR_INVALID_PARAM`:**
	An unknown operation type was provided in the update commands; or
	unknown or incompatible flags were provided in a flags parameter; or
	there was another problem with the provided parameters.
**Error `WIMLIB_ERR_INVALID_REPARSE_DATA`:**
	While executing an add command, a reparse point had invalid data.
**Error `WIMLIB_ERR_IS_DIRECTORY`:**
	An add command attempted to replace a directory with a non-directory; or
	a delete command without `WIMLIB_DELETE_FLAG_RECURSIVE` attempted to
	delete a directory; or a rename command attempted to rename a directory
	to a non-directory.
**Error `WIMLIB_ERR_NOTDIR`:**
	An add command attempted to replace a non-directory with a directory; or
	an add command attempted to set the root of the image to a
	non-directory; or a rename command attempted to rename a directory to a
	non-directory; or a component of an image path that was used as a
	directory was not, in fact, a directory.
**Error `WIMLIB_ERR_NOTEMPTY`:**
	A rename command attempted to rename a directory to a non-empty
	directory; or a rename command would have created a loop.
**Error `WIMLIB_ERR_NTFS_3G`:**
	While executing an add command with `WIMLIB_ADD_FLAG_NTFS` specified, an
	error occurred while reading data from the NTFS volume using libntfs-3g.
**Error `WIMLIB_ERR_OPEN`:**
	Failed to open a file to be captured while executing an add command.
**Error `WIMLIB_ERR_OPENDIR`:**
	Failed to open a directory to be captured while executing an add
	command.
**Error `WIMLIB_ERR_PATH_DOES_NOT_EXIST`:**
	A delete command without `WIMLIB_DELETE_FLAG_FORCE` specified was for a
	WIM path that did not exist; or a rename command attempted to rename a
	file that does not exist.
**Error `WIMLIB_ERR_READ`:**
	While executing an add command, failed to read data from a file or
	directory to be captured.
**Error `WIMLIB_ERR_READLINK`:**
	While executing an add command, failed to read the target of a symbolic
	link, junction, or other reparse point.
**Error `WIMLIB_ERR_STAT`:**
	While executing an add command, failed to read metadata for a file or
	directory.
**Error `WIMLIB_ERR_UNABLE_TO_READ_CAPTURE_CONFIG`:**
	A capture configuration file could not be read.
**Error `WIMLIB_ERR_UNSUPPORTED`:**
	A command had flags provided that are not supported on this platform or
	in this build of the library.
**Error `WIMLIB_ERR_UNSUPPORTED_FILE`:**
	An add command with `WIMLIB_ADD_FLAG_NO_UNSUPPORTED_EXCLUDE` specified
	discovered a file that was not of a supported type.

This function can additionally return `WIMLIB_ERR_DECOMPRESSION`,
`WIMLIB_ERR_INVALID_METADATA_RESOURCE`, `WIMLIB_ERR_METADATA_NOT_FOUND`,
`WIMLIB_ERR_READ`, or `WIMLIB_ERR_UNEXPECTED_END_OF_FILE`, all of which
indicate failure (for different reasons) to read the metadata resource for an
image that needed to be updated.

## Extraction

### `wimlib_extract_image`

Source: [include/wimlib.h:3019](/tmp/wimlib/include/wimlib.h:3019).

```c
int
wimlib_extract_image(WIMStruct *wim, int image,
		     const wimlib_tchar *target, int extract_flags);
```

Extract an image, or all images, from a `WIMStruct`.

The exact behavior of how wimlib extracts files from a WIM image is
controllable by the `extract_flags` parameter, but there also are
differences depending on the platform (UNIX-like vs Windows).  See the
documentation for wimapply for more information, including about the
NTFS-3G extraction mode.

**Parameter `wim`:**
	The WIM from which to extract the image(s), specified as a pointer to the
	`WIMStruct` for a standalone WIM file, a delta WIM file, or part 1 of a
	split WIM.  In the case of a WIM file that is not standalone, this
	`WIMStruct` must have had any needed external resources previously
	referenced using wimlib_reference_resources() or
	wimlib_reference_resource_files().
**Parameter `image`:**
	The 1-based index of the image to extract, or `WIMLIB_ALL_IMAGES` to
	extract all images.  Note: `WIMLIB_ALL_IMAGES` is unsupported in NTFS-3G
	extraction mode.
**Parameter `target`:**
	A null-terminated string which names the location to which the image(s)
	will be extracted.  By default, this is interpreted as a path to a
	directory.  Alternatively, if `WIMLIB_EXTRACT_FLAG_NTFS` is specified in
	`extract_flags,` then this is interpreted as a path to an unmounted
	NTFS volume.
**Parameter `extract_flags`:**
	Bitwise OR of flags prefixed with WIMLIB_EXTRACT_FLAG.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_DECOMPRESSION`:**
	The WIM file contains invalid compressed data.
**Error `WIMLIB_ERR_INVALID_IMAGE`:**
	`image` does not exist in `wim.`
**Error `WIMLIB_ERR_INVALID_METADATA_RESOURCE`:**
	The metadata for an image to extract was invalid.
**Error `WIMLIB_ERR_INVALID_PARAM`:**
	The extraction flags were invalid; more details may be found in the
	documentation for the specific extraction flags that were specified.  Or
	`target` was `NULL` or an empty string, or `wim` was `NULL.`
**Error `WIMLIB_ERR_INVALID_RESOURCE_HASH`:**
	The data of a file that needed to be extracted was corrupt.
**Error `WIMLIB_ERR_LINK`:**
	Failed to create a symbolic link or a hard link.
**Error `WIMLIB_ERR_METADATA_NOT_FOUND`:**
	`wim` does not contain image metadata; for example, it represents a
	non-first part of a split WIM.
**Error `WIMLIB_ERR_MKDIR`:**
	Failed create a directory.
**Error `WIMLIB_ERR_NTFS_3G`:**
	libntfs-3g reported that a problem occurred while writing to the NTFS
	volume.
**Error `WIMLIB_ERR_OPEN`:**
	Could not create a file, or failed to open an already-extracted file.
**Error `WIMLIB_ERR_READ`:**
	Failed to read data from the WIM.
**Error `WIMLIB_ERR_READLINK`:**
	Failed to determine the target of a symbolic link in the WIM.
**Error `WIMLIB_ERR_REPARSE_POINT_FIXUP_FAILED`:**
	Failed to fix the target of an absolute symbolic link (e.g. if the
	target would have exceeded the maximum allowed length).  (Only if
	reparse data was supported by the extraction mode and
	`WIMLIB_EXTRACT_FLAG_STRICT_SYMLINKS` was specified in `extract_flags.)`
**Error `WIMLIB_ERR_RESOURCE_NOT_FOUND`:**
	A file data blob that needed to be extracted could not be found in the
	blob lookup table of `wim.`  See `G_nonstandalone_wims` (upstream header section).
**Error `WIMLIB_ERR_SET_ATTRIBUTES`:**
	Failed to set attributes on a file.
**Error `WIMLIB_ERR_SET_REPARSE_DATA`:**
	Failed to set reparse data on a file (only if reparse data was supported
	by the extraction mode).
**Error `WIMLIB_ERR_SET_SECURITY`:**
	Failed to set security descriptor on a file.
**Error `WIMLIB_ERR_SET_SHORT_NAME`:**
	Failed to set the short name of a file.
**Error `WIMLIB_ERR_SET_TIMESTAMPS`:**
	Failed to set timestamps on a file.
**Error `WIMLIB_ERR_UNEXPECTED_END_OF_FILE`:**
	Unexpected end-of-file occurred when reading data from the WIM.
**Error `WIMLIB_ERR_UNSUPPORTED`:**
	A requested extraction flag, or the data or metadata that must be
	extracted to support it, is unsupported in the build and configuration
	of wimlib, or on the current platform or extraction mode or target
	volume.  Flags affected by this include `WIMLIB_EXTRACT_FLAG_NTFS`,
	`WIMLIB_EXTRACT_FLAG_UNIX_DATA`, `WIMLIB_EXTRACT_FLAG_STRICT_ACLS`,
	`WIMLIB_EXTRACT_FLAG_STRICT_SHORT_NAMES`,
	`WIMLIB_EXTRACT_FLAG_STRICT_TIMESTAMPS`, and
	`WIMLIB_EXTRACT_FLAG_STRICT_SYMLINKS`.  For example, if
	`WIMLIB_EXTRACT_FLAG_STRICT_SHORT_NAMES` is specified in `extract_flags,` `WIMLIB_ERR_UNSUPPORTED` will be returned if the WIM
	image contains one or more files with short names, but extracting short
	names is not supported --- on Windows, this occurs if the target volume
	does not support short names, while on non-Windows, this occurs if
	`WIMLIB_EXTRACT_FLAG_NTFS` was not specified in `extract_flags.`
**Error `WIMLIB_ERR_WIMBOOT`:**
	`WIMLIB_EXTRACT_FLAG_WIMBOOT` was specified in `extract_flags,` but
	there was a problem creating WIMBoot pointer files or registering a
	source WIM file with the Windows Overlay Filesystem (WOF) driver.
**Error `WIMLIB_ERR_WRITE`:**
	Failed to write data to a file being extracted.

If a progress function is registered with `wim,` then as each image is
extracted it will receive `WIMLIB_PROGRESS_MSG_EXTRACT_IMAGE_BEGIN`, then
zero or more `WIMLIB_PROGRESS_MSG_EXTRACT_FILE_STRUCTURE` messages, then zero
or more `WIMLIB_PROGRESS_MSG_EXTRACT_STREAMS` messages, then zero or more
`WIMLIB_PROGRESS_MSG_EXTRACT_METADATA` messages, then
`WIMLIB_PROGRESS_MSG_EXTRACT_IMAGE_END`.

### `wimlib_extract_image_from_pipe`

Source: [include/wimlib.h:3061](/tmp/wimlib/include/wimlib.h:3061).

```c
int
wimlib_extract_image_from_pipe(int pipe_fd,
			       const wimlib_tchar *image_num_or_name,
			       const wimlib_tchar *target, int extract_flags);
```

Extract one image from a pipe on which a pipable WIM is being sent.

See the documentation for `WIMLIB_WRITE_FLAG_PIPABLE`, and `subsec_pipable_wims` (upstream header section), for more information about pipable WIMs.

This function operates in a special way to read the WIM fully sequentially.
As a result, there is no `WIMStruct` is made visible to library users, and
you cannot call wimlib_open_wim() on the pipe.  (You can, however, use
wimlib_open_wim() to transparently open a pipable WIM if it's available as a
seekable file, not a pipe.)

**Parameter `pipe_fd`:**
	File descriptor, which may be a pipe, opened for reading and positioned
	at the start of the pipable WIM.
**Parameter `image_num_or_name`:**
	String that specifies the 1-based index or name of the image to extract.
	It is translated to an image index using the same rules that
	wimlib_resolve_image() uses.  However, unlike wimlib_extract_image(),
	only a single image (not all images) can be specified.  Alternatively,
	specify `NULL` here to use the first image in the WIM if it contains
	exactly one image but otherwise return `WIMLIB_ERR_INVALID_IMAGE`.
**Parameter `target`:**
	Same as the corresponding parameter to wimlib_extract_image().
**Parameter `extract_flags`:**
	Same as the corresponding parameter to wimlib_extract_image().

**Return:** 0 on success; a `wimlib_error_code` value on failure.  The possible
error codes include those returned by wimlib_extract_image() and
wimlib_open_wim() as well as the following:

**Error `WIMLIB_ERR_INVALID_PIPABLE_WIM`:**
	Data read from the pipable WIM was invalid.
**Error `WIMLIB_ERR_NOT_PIPABLE`:**
	The WIM being piped over `pipe_fd` is a normal WIM, not a pipable WIM.

### `wimlib_extract_image_from_pipe_with_progress`

Source: [include/wimlib.h:3075](/tmp/wimlib/include/wimlib.h:3075).

```c
int
wimlib_extract_image_from_pipe_with_progress(int pipe_fd,
					     const wimlib_tchar *image_num_or_name,
					     const wimlib_tchar *target,
					     int extract_flags,
					     wimlib_progress_func_t progfunc,
					     void *progctx);
```

Same as wimlib_extract_image_from_pipe(), but allows specifying a progress
function.  The progress function will be used while extracting the image and
will receive the normal extraction progress messages, such as
`WIMLIB_PROGRESS_MSG_EXTRACT_STREAMS`, in addition to
`WIMLIB_PROGRESS_MSG_EXTRACT_SPWM_PART_BEGIN`.

### `wimlib_extract_pathlist`

Source: [include/wimlib.h:3102](/tmp/wimlib/include/wimlib.h:3102).

```c
int
wimlib_extract_pathlist(WIMStruct *wim, int image,
			const wimlib_tchar *target,
			const wimlib_tchar *path_list_file,
			int extract_flags);
```

Similar to wimlib_extract_paths(), but the paths to extract from the WIM
image are specified in the ASCII, UTF-8, or UTF-16LE text file named by `path_list_file` which itself contains the list of paths to use, one per line.
Leading and trailing whitespace is ignored.  Empty lines and lines beginning
with the ';' or '#' characters are ignored.  No quotes are needed, as paths
are otherwise delimited by the newline character.  However, quotes will be
stripped if present.

If `path_list_file` is `NULL,` then the pathlist file is read from standard
input.

The error codes are the same as those returned by wimlib_extract_paths(),
except that wimlib_extract_pathlist() returns an appropriate error code if it
cannot read the path list file (e.g. `WIMLIB_ERR_OPEN`, `WIMLIB_ERR_STAT`,
`WIMLIB_ERR_READ`).

### `wimlib_extract_paths`

Source: [include/wimlib.h:3177](/tmp/wimlib/include/wimlib.h:3177).

```c
int
wimlib_extract_paths(WIMStruct *wim,
		     int image,
		     const wimlib_tchar *target,
		     const wimlib_tchar * const *paths,
		     size_t num_paths,
		     int extract_flags);
```

Extract zero or more paths (files or directory trees) from the specified WIM
image.

By default, each path will be extracted to a corresponding subdirectory of
the target based on its location in the image.  For example, if one of the
paths to extract is `/Windows/explorer.exe` and the target is
`outdir`, the file will be extracted to
`outdir/Windows/explorer.exe`.  This behavior can be changed by
providing the flag `WIMLIB_EXTRACT_FLAG_NO_PRESERVE_DIR_STRUCTURE`, which
will cause each file or directory tree to be placed directly in the target
directory --- so the same example would extract `/Windows/explorer.exe`
to `outdir/explorer.exe`.

With globbing turned off (the default), paths are always checked for
existence strictly; that is, if any path to extract does not exist in the
image, then nothing is extracted and the function fails with
`WIMLIB_ERR_PATH_DOES_NOT_EXIST`.  But with globbing turned on
(`WIMLIB_EXTRACT_FLAG_GLOB_PATHS` specified), globs are by default permitted
to match no files, and there is a flag (`WIMLIB_EXTRACT_FLAG_STRICT_GLOB`) to
enable the strict behavior if desired.

Symbolic links are not dereferenced when paths in the image are interpreted.

**Parameter `wim`:**
	WIM from which to extract the paths, specified as a pointer to the
	`WIMStruct` for a standalone WIM file, a delta WIM file, or part 1 of a
	split WIM.  In the case of a WIM file that is not standalone, this
	`WIMStruct` must have had any needed external resources previously
	referenced using wimlib_reference_resources() or
	wimlib_reference_resource_files().
**Parameter `image`:**
	The 1-based index of the WIM image from which to extract the paths.
**Parameter `paths`:**
	Array of paths to extract.  Each element must be the absolute path to a
	file or directory within the image.  Path separators may be either
	forwards or backwards slashes, and leading path separators are optional.
	The paths will be interpreted either case-sensitively (UNIX default) or
	case-insensitively (Windows default); however, the case sensitivity can
	be configured explicitly at library initialization time by passing an
	appropriate flag to wimlib_global_init().
	<br/>
	By default, "globbing" is disabled, so the characters `*` and `?` are
	interpreted literally.  This can be changed by specifying
	`WIMLIB_EXTRACT_FLAG_GLOB_PATHS` in `extract_flags.`
**Parameter `num_paths`:**
	Number of paths specified in `paths.`
**Parameter `target`:**
	Directory to which to extract the paths.
**Parameter `extract_flags`:**
	Bitwise OR of flags prefixed with WIMLIB_EXTRACT_FLAG.

**Return:** 0 on success; a `wimlib_error_code` value on failure.  Most of the
error codes are the same as those returned by wimlib_extract_image().  Below,
some of the error codes returned in situations specific to path-mode
extraction are documented:

**Error `WIMLIB_ERR_NOT_A_REGULAR_FILE`:**
	`WIMLIB_EXTRACT_FLAG_TO_STDOUT` was specified in `extract_flags,` but
	one of the paths to extract did not name a regular file.
**Error `WIMLIB_ERR_PATH_DOES_NOT_EXIST`:**
	One of the paths to extract does not exist in the image; see discussion
	above about strict vs. non-strict behavior.

If a progress function is registered with `wim,` then it will receive
`WIMLIB_PROGRESS_MSG_EXTRACT_STREAMS`.

### `wimlib_extract_xml_data`

Source: [include/wimlib.h:3199](/tmp/wimlib/include/wimlib.h:3199).

```c
int
wimlib_extract_xml_data(WIMStruct *wim, FILE *fp);
```

Similar to wimlib_get_xml_data(), but the XML document will be written to the
specified standard C `FILE*` instead of retrieved in an in-memory
buffer.

**Return:** 0 on success; a `wimlib_error_code` value on failure.  This may
return any error code which can be returned by wimlib_get_xml_data() as well
as the following error codes:

**Error `WIMLIB_ERR_WRITE`:**
	Failed to write the data to the requested file.

## Information and iteration

### `wimlib_get_compression_type_string`

Source: [include/wimlib.h:3230](/tmp/wimlib/include/wimlib.h:3230).

```c
const wimlib_tchar *
wimlib_get_compression_type_string(enum wimlib_compression_type ctype);
```

Convert a `wimlib_compression_type` value into a string.

**Parameter `ctype`:**
	The compression type value to convert.

**Return:**
	A statically allocated string naming the compression type, such as
	"None", "LZX", or "XPRESS".  If the value was unrecognized, then
	the resulting string will be "Invalid".

### `wimlib_get_error_string`

Source: [include/wimlib.h:3246](/tmp/wimlib/include/wimlib.h:3246).

```c
const wimlib_tchar *
wimlib_get_error_string(enum wimlib_error_code code);
```

Convert a wimlib error code into a string describing it.

**Parameter `code`:**
	An error code returned by one of wimlib's functions.

**Return:**
	Pointer to a statically allocated string describing the error code.  If
	the value was unrecognized, then the resulting string will be "Unknown
	error".

### `wimlib_get_image_description`

Source: [include/wimlib.h:3255](/tmp/wimlib/include/wimlib.h:3255).

```c
const wimlib_tchar *
wimlib_get_image_description(const WIMStruct *wim, int image);
```

Get the description of the specified image.  Equivalent to
`wimlib_get_image_property(wim, image, "DESCRIPTION")`.

### `wimlib_get_image_name`

Source: [include/wimlib.h:3266](/tmp/wimlib/include/wimlib.h:3266).

```c
const wimlib_tchar *
wimlib_get_image_name(const WIMStruct *wim, int image);
```

Get the name of the specified image.  Equivalent to
`wimlib_get_image_property(wim, image, "NAME")`, except that
wimlib_get_image_name() will return an empty string if the image is unnamed
whereas wimlib_get_image_property() may return `NULL` in that case.

### `wimlib_get_image_property`

Source: [include/wimlib.h:3297](/tmp/wimlib/include/wimlib.h:3297).

```c
const wimlib_tchar *
wimlib_get_image_property(const WIMStruct *wim, int image,
			  const wimlib_tchar *property_name);
```

Since wimlib v1.8.3: get a per-image property from the WIM's XML document.
This is an alternative to wimlib_get_image_name() and
wimlib_get_image_description() which allows getting any simple string
property.

**Parameter `wim`:**
	Pointer to the `WIMStruct` for the WIM.
**Parameter `image`:**
	The 1-based index of the image for which to get the property.
**Parameter `property_name`:**
	The name of the image property, for example "NAME", "DESCRIPTION", or
	"TOTALBYTES".  The name can contain forward slashes to indicate a nested
	XML element; for example, "WINDOWS/VERSION/BUILD" indicates the BUILD
	element nested within the VERSION element nested within the WINDOWS
	element.  Since wimlib v1.9.0, a bracketed number can be used to
	indicate one of several identically-named elements; for example,
	"WINDOWS/LANGUAGES/LANGUAGE[2]" indicates the second "LANGUAGE" element
	nested within the "WINDOWS/LANGUAGES" element.  Note that element names
	are case sensitive.

**Return:**
	The property's value as a `wimlib_tchar` string, or `NULL` if there is
	no such property.  The string may not remain valid after later library
	calls, so the caller should duplicate it if needed.

### `wimlib_get_version`

Source: [include/wimlib.h:3312](/tmp/wimlib/include/wimlib.h:3312).

```c
uint32_t
wimlib_get_version(void);
```

Return the version of wimlib as a 32-bit number whose top 12 bits contain the
major version, the next 10 bits contain the minor version, and the low 10
bits contain the patch version.

In other words, the returned value is equal to `((WIMLIB_MAJOR_VERSION <<
20) | (WIMLIB_MINOR_VERSION << 10) | WIMLIB_PATCH_VERSION)` for the
corresponding header file.

### `wimlib_get_version_string`

Source: [include/wimlib.h:3322](/tmp/wimlib/include/wimlib.h:3322).

```c
const wimlib_tchar *
wimlib_get_version_string(void);
```

Since wimlib v1.13.0: like wimlib_get_version(), but returns the full
PACKAGE_VERSION string that was set at build time.  (This allows a beta
release to be distinguished from an official release.)

### `wimlib_get_wim_info`

Source: [include/wimlib.h:3339](/tmp/wimlib/include/wimlib.h:3339).

```c
int
wimlib_get_wim_info(WIMStruct *wim, struct wimlib_wim_info *info);
```

Get basic information about a WIM file.

**Parameter `wim`:**
	Pointer to the `WIMStruct` to query.  This need not represent a
	standalone WIM (e.g. it could represent part of a split WIM).
**Parameter `info`:**
	A `wimlib_wim_info` structure that will be filled in with information
	about the WIM file.

**Return:** 0

### `wimlib_get_xml_data`

Source: [include/wimlib.h:3369](/tmp/wimlib/include/wimlib.h:3369).

```c
int
wimlib_get_xml_data(WIMStruct *wim, void **buf_ret, size_t *bufsize_ret);
```

Read a WIM file's XML document into an in-memory buffer.

The XML document contains metadata about the WIM file and the images stored
in it.

**Parameter `wim`:**
	Pointer to the `WIMStruct` to query.  This need not represent a
	standalone WIM (e.g. it could represent part of a split WIM).
**Parameter `buf_ret`:**
	On success, a pointer to an allocated buffer containing the raw UTF16-LE
	XML document is written to this location.
**Parameter `bufsize_ret`:**
	The size of the XML document in bytes is written to this location.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_NO_FILENAME`:**
	`wim` is not backed by a file and therefore does not have an XML
	document.
**Error `WIMLIB_ERR_READ`:**
	Failed to read the XML document from the WIM file.
**Error `WIMLIB_ERR_UNEXPECTED_END_OF_FILE`:**
	Failed to read the XML document from the WIM file.

### `wimlib_image_name_in_use`

Source: [include/wimlib.h:3418](/tmp/wimlib/include/wimlib.h:3418).

```c
bool
wimlib_image_name_in_use(const WIMStruct *wim, const wimlib_tchar *name);
```

Determine if an image name is already used by some image in the WIM.

**Parameter `wim`:**
	Pointer to the `WIMStruct` to query.  This need not represent a
	standalone WIM (e.g. it could represent part of a split WIM).
**Parameter `name`:**
	The name to check.

**Return:**
	`true` if there is already an image in `wim` named `name;` `false`
	if there is no image named `name` in `wim.`  If `name` is `NULL` or
	the empty string, then `false` is returned.

### `wimlib_iterate_dir_tree`

Source: [include/wimlib.h:3465](/tmp/wimlib/include/wimlib.h:3465).

```c
int
wimlib_iterate_dir_tree(WIMStruct *wim, int image, const wimlib_tchar *path,
			int flags,
			wimlib_iterate_dir_tree_callback_t cb, void *user_ctx);
```

Iterate through a file or directory tree in a WIM image.  By specifying
appropriate flags and a callback function, you can get the attributes of a
file in the image, get a directory listing, or even get a listing of the
entire image.

**Parameter `wim`:**
	The `WIMStruct` containing the image(s) over which to iterate.  This
	`WIMStruct` must contain image metadata, so it cannot be the non-first
	part of a split WIM (for example).
**Parameter `image`:**
	The 1-based index of the image that contains the files or directories to
	iterate over, or `WIMLIB_ALL_IMAGES` to iterate over all images.
**Parameter `path`:**
	Path in the image at which to do the iteration.
**Parameter `flags`:**
	Bitwise OR of flags prefixed with WIMLIB_ITERATE_DIR_TREE_FLAG.
**Parameter `cb`:**
	A callback function that will receive each directory entry.
**Parameter `user_ctx`:**
	An extra parameter that will always be passed to the callback function
	`cb.`

**Return:** Normally, returns 0 if all calls to `cb` returned 0; otherwise the
first nonzero value that was returned from `cb.`  However, additional
`wimlib_error_code` values may be returned, including the following:

**Error `WIMLIB_ERR_INVALID_IMAGE`:**
	`image` does not exist in `wim.`
**Error `WIMLIB_ERR_PATH_DOES_NOT_EXIST`:**
	`path` does not exist in the image.
**Error `WIMLIB_ERR_RESOURCE_NOT_FOUND`:**
	`WIMLIB_ITERATE_DIR_TREE_FLAG_RESOURCES_NEEDED` was specified, but the
	data for some files could not be found in the blob lookup table of `wim.`

This function can additionally return `WIMLIB_ERR_DECOMPRESSION`,
`WIMLIB_ERR_INVALID_METADATA_RESOURCE`, `WIMLIB_ERR_METADATA_NOT_FOUND`,
`WIMLIB_ERR_READ`, or `WIMLIB_ERR_UNEXPECTED_END_OF_FILE`, all of which
indicate failure (for different reasons) to read the metadata resource for an
image over which iteration needed to be done.

### `wimlib_iterate_lookup_table`

Source: [include/wimlib.h:3499](/tmp/wimlib/include/wimlib.h:3499).

```c
int
wimlib_iterate_lookup_table(WIMStruct *wim, int flags,
			    wimlib_iterate_lookup_table_callback_t cb,
			    void *user_ctx);
```

Iterate through the blob lookup table of a `WIMStruct`.  This can be used to
directly get a listing of the unique "blobs" contained in a WIM file, which
are deduplicated over all images.

Specifically, each listed blob may be from any of the following sources:

- Metadata blobs, if the `WIMStruct` contains image metadata
- File blobs from the on-disk WIM file (if any) backing the `WIMStruct`
- File blobs from files that have been added to the in-memory `WIMStruct`,
  e.g. by using wimlib_add_image()
- File blobs from external WIMs referenced by
  wimlib_reference_resource_files() or wimlib_reference_resources()

**Parameter `wim`:**
	Pointer to the `WIMStruct` for which to get the blob listing.
**Parameter `flags`:**
	Reserved; set to 0.
**Parameter `cb`:**
	A callback function that will receive each blob.
**Parameter `user_ctx`:**
	An extra parameter that will always be passed to the callback function
	`cb.`

**Return:** 0 if all calls to `cb` returned 0; otherwise the first nonzero value
that was returned from `cb.`

### `wimlib_print_available_images`

Source: [include/wimlib.h:3842](/tmp/wimlib/include/wimlib.h:3842).

```c
void
wimlib_print_available_images(const WIMStruct *wim, int image);
```

(Deprecated) Print information about one image, or all images, contained in a
WIM.

**Parameter `wim`:**
	Pointer to the `WIMStruct` to query.  This need not represent a
	standalone WIM (e.g. it could represent part of a split WIM).
**Parameter `image`:**
	The 1-based index of the image for which to print information, or
	`WIMLIB_ALL_IMAGES` to print information about all images.

**Return:** This function has no return value.  No error checking is done when
printing the information.  If `image` is invalid, an error message is
printed.

This function is deprecated; use wimlib_get_xml_data() or
wimlib_get_image_property() to query image information instead.

### `wimlib_print_header`

Source: [include/wimlib.h:3850](/tmp/wimlib/include/wimlib.h:3850).

```c
void
wimlib_print_header(const WIMStruct *wim);
```

Print the header of the WIM file (intended for debugging only).

### `wimlib_resolve_image`

Source: [include/wimlib.h:4055](/tmp/wimlib/include/wimlib.h:4055).

```c
int
wimlib_resolve_image(WIMStruct *wim,
		     const wimlib_tchar *image_name_or_num);
```

Translate a string specifying the name or number of an image in the WIM into
the number of the image.  The images are numbered starting at 1.

**Parameter `wim`:**
	Pointer to the `WIMStruct` for a WIM.
**Parameter `image_name_or_num`:**
	A string specifying the name or number of an image in the WIM.  If it
	parses to a positive integer, this integer is taken to specify the
	number of the image, indexed starting at 1.  Otherwise, it is taken to
	be the name of an image, as given in the XML data for the WIM file.  It
	also may be the keyword "all" or the string "*", both of which will
	resolve to `WIMLIB_ALL_IMAGES`.
	<br/> <br/>
	There is no way to search for an image actually named "all", "*", or an
	integer number, or an image that has no name.  However, you can use
	wimlib_get_image_name() to get the name of any image.

**Return:**
	If the string resolved to a single existing image, the number of that
	image, indexed starting at 1, is returned.  If the keyword "all" or "*"
	was specified, `WIMLIB_ALL_IMAGES` is returned.  Otherwise,
	`WIMLIB_NO_IMAGE` is returned.  If `image_name_or_num` was `NULL` or
	the empty string, `WIMLIB_NO_IMAGE` is returned, even if one or more
	images in `wim` has no name.  (Since a WIM may have multiple unnamed
	images, an unnamed image must be specified by index to eliminate the
	ambiguity.)

## Writing, splitting, joining and verification

### `wimlib_join`

Source: [include/wimlib.h:3540](/tmp/wimlib/include/wimlib.h:3540).

```c
int
wimlib_join(const wimlib_tchar * const *swms,
	    unsigned num_swms,
	    const wimlib_tchar *output_path,
	    int swm_open_flags,
	    int wim_write_flags);
```

Join a split WIM into a stand-alone (one-part) WIM.

**Parameter `swms`:**
	An array of strings that gives the filenames of all parts of the split
	WIM.  No specific order is required, but all parts must be included with
	no duplicates.
**Parameter `num_swms`:**
	Number of filenames in `swms.`
**Parameter `swm_open_flags`:**
	Open flags for the split WIM parts (e.g.
	`WIMLIB_OPEN_FLAG_CHECK_INTEGRITY`).
**Parameter `wim_write_flags`:**
	Bitwise OR of relevant flags prefixed with WIMLIB_WRITE_FLAG, which will
	be used to write the joined WIM.
**Parameter `output_path`:**
	The path to write the joined WIM file to.

**Return:** 0 on success; a `wimlib_error_code` value on failure.  This function
may return most error codes that can be returned by wimlib_open_wim() and
wimlib_write(), as well as the following error codes:

**Error `WIMLIB_ERR_SPLIT_INVALID`:**
	The split WIMs do not form a valid WIM because they do not include all
	the parts of the original WIM, there are duplicate parts, or not all the
	parts have the same GUID and compression type.

Note: wimlib is generalized enough that this function is not actually needed
to join a split WIM; instead, you could open the first part of the split WIM,
then reference the other parts with wimlib_reference_resource_files(), then
write the joined WIM using wimlib_write().  However, wimlib_join() provides
an easy-to-use wrapper around this that has some advantages (e.g.  extra
sanity checks).

### `wimlib_join_with_progress`

Source: [include/wimlib.h:3558](/tmp/wimlib/include/wimlib.h:3558).

```c
int
wimlib_join_with_progress(const wimlib_tchar * const *swms,
			  unsigned num_swms,
			  const wimlib_tchar *output_path,
			  int swm_open_flags,
			  int wim_write_flags,
			  wimlib_progress_func_t progfunc,
			  void *progctx);
```

Same as wimlib_join(), but allows specifying a progress function.  The
progress function will receive the write progress messages, such as
`WIMLIB_PROGRESS_MSG_WRITE_STREAMS`, while writing the joined WIM.  In
addition, if `WIMLIB_OPEN_FLAG_CHECK_INTEGRITY` is specified in `swm_open_flags,` the progress function will receive a series of
`WIMLIB_PROGRESS_MSG_VERIFY_INTEGRITY` messages when each of the split WIM
parts is opened.

### `wimlib_overwrite`

Source: [include/wimlib.h:3819](/tmp/wimlib/include/wimlib.h:3819).

```c
int
wimlib_overwrite(WIMStruct *wim, int write_flags, unsigned num_threads);
```

Commit a `WIMStruct` to disk, updating its backing file.

There are several alternative ways in which changes may be committed:

  1. Full rebuild: write the updated WIM to a temporary file, then rename the
	temporary file to the original.
  2. Appending: append updates to the new original WIM file, then overwrite
	its header such that those changes become visible to new readers.
  3. Compaction: normally should not be used; see
	`WIMLIB_WRITE_FLAG_UNSAFE_COMPACT` for details.

Append mode is often much faster than a full rebuild, but it wastes some
amount of space due to leaving "holes" in the WIM file.  Because of the
greater efficiency, wimlib_overwrite() normally defaults to append mode.
However, `WIMLIB_WRITE_FLAG_REBUILD` can be used to explicitly request a full
rebuild.  In addition, if wimlib_delete_image() has been used on the
`WIMStruct`, then the default mode switches to rebuild mode, and
`WIMLIB_WRITE_FLAG_SOFT_DELETE` can be used to explicitly request append
mode.

If this function completes successfully, then no more functions can be called
on the `WIMStruct` other than wimlib_free().  If you need to continue using
the WIM file, you must use wimlib_open_wim() to open a new `WIMStruct` for
it.

**Parameter `wim`:**
	Pointer to a `WIMStruct` to commit to its backing file.
**Parameter `write_flags`:**
	Bitwise OR of relevant flags prefixed with WIMLIB_WRITE_FLAG.
**Parameter `num_threads`:**
	The number of threads to use for compressing data, or 0 to have the
	library automatically choose an appropriate number.

**Return:** 0 on success; a `wimlib_error_code` value on failure.  This function
may return most error codes returned by wimlib_write() as well as the
following error codes:

**Error `WIMLIB_ERR_ALREADY_LOCKED`:**
	Another process is currently modifying the WIM file.
**Error `WIMLIB_ERR_NO_FILENAME`:**
	`wim` is not backed by an on-disk file.  In other words, it is a
	`WIMStruct` created by wimlib_create_new_wim() rather than
	wimlib_open_wim().
**Error `WIMLIB_ERR_RENAME`:**
	The temporary file to which the WIM was written could not be renamed to
	the original file.
**Error `WIMLIB_ERR_WIM_IS_READONLY`:**
	The WIM file is considered read-only because of any of the reasons
	mentioned in the documentation for the `WIMLIB_OPEN_FLAG_WRITE_ACCESS`
	flag.

If a progress function is registered with `wim,` then it will receive the
messages `WIMLIB_PROGRESS_MSG_WRITE_STREAMS`,
`WIMLIB_PROGRESS_MSG_WRITE_METADATA_BEGIN`, and
`WIMLIB_PROGRESS_MSG_WRITE_METADATA_END`.

### `wimlib_set_output_chunk_size`

Source: [include/wimlib.h:4224](/tmp/wimlib/include/wimlib.h:4224).

```c
int
wimlib_set_output_chunk_size(WIMStruct *wim, uint32_t chunk_size);
```

Set a `WIMStruct`'s output compression chunk size.  This is the compression
chunk size that will be used for writing non-solid resources in subsequent
calls to wimlib_write() or wimlib_overwrite().  A larger compression chunk
size often results in a better compression ratio, but compression may be
slower and the speed of random access to data may be reduced.  In addition,
some chunk sizes are not compatible with Microsoft software.

**Parameter `wim`:**
	The `WIMStruct` for which to set the output chunk size.
**Parameter `chunk_size`:**
	The chunk size (in bytes) to set.  The valid chunk sizes are dependent
	on the compression type.  See the documentation for each
	`wimlib_compression_type` constant for more information.  As a special
	case, if `chunk_size` is specified as 0, then the chunk size will be
	reset to the default for the currently selected output compression type.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_INVALID_CHUNK_SIZE`:**
	`chunk_size` was not 0 or a supported chunk size for the currently
	selected output compression type.

### `wimlib_set_output_pack_chunk_size`

Source: [include/wimlib.h:4233](/tmp/wimlib/include/wimlib.h:4233).

```c
int
wimlib_set_output_pack_chunk_size(WIMStruct *wim, uint32_t chunk_size);
```

Similar to wimlib_set_output_chunk_size(), but set the chunk size for writing
solid resources.

### `wimlib_set_output_compression_type`

Source: [include/wimlib.h:4255](/tmp/wimlib/include/wimlib.h:4255).

```c
int
wimlib_set_output_compression_type(WIMStruct *wim,
				   enum wimlib_compression_type ctype);
```

Set a `WIMStruct`'s output compression type.  This is the compression type
that will be used for writing non-solid resources in subsequent calls to
wimlib_write() or wimlib_overwrite().

**Parameter `wim`:**
	The `WIMStruct` for which to set the output compression type.
**Parameter `ctype`:**
	The compression type to set.  If this compression type is incompatible
	with the current output chunk size, then the output chunk size will be
	reset to the default for the new compression type.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_INVALID_COMPRESSION_TYPE`:**
	`ctype` did not specify a valid compression type.

### `wimlib_set_output_pack_compression_type`

Source: [include/wimlib.h:4265](/tmp/wimlib/include/wimlib.h:4265).

```c
int
wimlib_set_output_pack_compression_type(WIMStruct *wim,
					enum wimlib_compression_type ctype);
```

Similar to wimlib_set_output_compression_type(), but set the compression type
for writing solid resources.  This cannot be `WIMLIB_COMPRESSION_TYPE_NONE`.

### `wimlib_split`

Source: [include/wimlib.h:4358](/tmp/wimlib/include/wimlib.h:4358).

```c
int
wimlib_split(WIMStruct *wim,
	     const wimlib_tchar *swm_name,
	     uint64_t part_size,
	     int write_flags);
```

Split a WIM into multiple parts.

**Parameter `wim`:**
	The `WIMStruct` for the WIM to split.
**Parameter `swm_name`:**
	Name of the split WIM (SWM) file to create.  This will be the name of
	the first part.  The other parts will, by default, have the same name
	with 2, 3, 4, ..., etc.  appended before the suffix.  However, the exact
	names can be customized using the progress function.
**Parameter `part_size`:**
	The maximum size per part, in bytes.  Unfortunately, it is not
	guaranteed that this will really be the maximum size per part, because
	some file resources in the WIM may be larger than this size, and the WIM
	file format provides no way to split up file resources among multiple
	WIMs.
**Parameter `write_flags`:**
	Bitwise OR of relevant flags prefixed with `WIMLIB_WRITE_FLAG.`  These
	flags will be used to write each split WIM part.  Specify 0 here to get
	the default behavior.

**Return:** 0 on success; a `wimlib_error_code` value on failure.  This function
may return most error codes that can be returned by wimlib_write() as well as
the following error codes:

**Error `WIMLIB_ERR_INVALID_PARAM`:**
	`swm_name` was not a nonempty string, or `part_size` was 0.
**Error `WIMLIB_ERR_UNSUPPORTED`:**
	The WIM contains solid resources.  Splitting a WIM containing solid
	resources is not supported.

If a progress function is registered with `wim,` then for each split WIM
part that is written it will receive the messages
`WIMLIB_PROGRESS_MSG_SPLIT_BEGIN_PART` and
`WIMLIB_PROGRESS_MSG_SPLIT_END_PART`.  Since wimlib v1.13.4 it will also
receive `WIMLIB_PROGRESS_MSG_WRITE_STREAMS` messages while writing each part;
these messages will report the progress of the current part only.

### `wimlib_verify_wim`

Source: [include/wimlib.h:4401](/tmp/wimlib/include/wimlib.h:4401).

```c
int
wimlib_verify_wim(WIMStruct *wim, int verify_flags);
```

Perform verification checks on a WIM file.

This function is intended for safety checking and/or debugging.  If used on a
well-formed WIM file, it should always succeed.

**Parameter `wim`:**
	The `WIMStruct` for the WIM file to verify.  Note: for an extra layer of
	verification, it is a good idea to have used
	`WIMLIB_OPEN_FLAG_CHECK_INTEGRITY` when you opened the file.
	<br/>
	If verifying a split WIM, specify the first part of the split WIM here,
	and reference the other parts using wimlib_reference_resource_files()
	before calling this function.
**Parameter `verify_flags`:**
	Reserved; must be 0.

**Return:** 0 if the WIM file was successfully verified; a `wimlib_error_code`
value if it failed verification or another error occurred.

**Error `WIMLIB_ERR_DECOMPRESSION`:**
	The WIM file contains invalid compressed data.
**Error `WIMLIB_ERR_INVALID_METADATA_RESOURCE`:**
	The metadata resource for an image is invalid.
**Error `WIMLIB_ERR_INVALID_RESOURCE_HASH`:**
	File data stored in the WIM file is corrupt.
**Error `WIMLIB_ERR_RESOURCE_NOT_FOUND`:**
	The data for a file in an image could not be found.  See `G_nonstandalone_wims` (upstream header section).

If a progress function is registered with `wim,` then it will receive the
following progress messages: `WIMLIB_PROGRESS_MSG_BEGIN_VERIFY_IMAGE`,
`WIMLIB_PROGRESS_MSG_END_VERIFY_IMAGE`, and
`WIMLIB_PROGRESS_MSG_VERIFY_STREAMS`.

### `wimlib_write`

Source: [include/wimlib.h:4612](/tmp/wimlib/include/wimlib.h:4612).

```c
int
wimlib_write(WIMStruct *wim,
	     const wimlib_tchar *path,
	     int image,
	     int write_flags,
	     unsigned num_threads);
```

Persist a `WIMStruct` to a new on-disk WIM file.

This brings in file data from any external locations, such as directory trees
or NTFS volumes scanned with wimlib_add_image(), or other WIM files via
wimlib_export_image(), and incorporates it into a new on-disk WIM file.

By default, the new WIM file is written as stand-alone.  Using the
`WIMLIB_WRITE_FLAG_SKIP_EXTERNAL_WIMS` flag, a "delta" WIM can be written
instead.  However, this function cannot directly write a "split" WIM; use
wimlib_split() for that.

**Parameter `wim`:**
	Pointer to the `WIMStruct` being persisted.
**Parameter `path`:**
	The path to the on-disk file to write.
**Parameter `image`:**
	Normally, specify `WIMLIB_ALL_IMAGES` here.  This indicates that all
	images are to be included in the new on-disk WIM file.  If for some
	reason you only want to include a single image, specify the 1-based
	index of that image instead.
**Parameter `write_flags`:**
	Bitwise OR of flags prefixed with `WIMLIB_WRITE_FLAG.`
**Parameter `num_threads`:**
	The number of threads to use for compressing data, or 0 to have the
	library automatically choose an appropriate number.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_CONCURRENT_MODIFICATION_DETECTED`:**
	A file that had previously been scanned for inclusion in the WIM was
	concurrently modified.
**Error `WIMLIB_ERR_INVALID_IMAGE`:**
	`image` did not exist in `wim.`
**Error `WIMLIB_ERR_INVALID_RESOURCE_HASH`:**
	A file, stored in another WIM, which needed to be written was corrupt.
**Error `WIMLIB_ERR_INVALID_PARAM`:**
	`path` was not a nonempty string, or invalid flags were passed.
**Error `WIMLIB_ERR_OPEN`:**
	Failed to open the output WIM file for writing, or failed to open a file
	whose data needed to be included in the WIM.
**Error `WIMLIB_ERR_READ`:**
	Failed to read data that needed to be included in the WIM.
**Error `WIMLIB_ERR_RESOURCE_NOT_FOUND`:**
	A file data blob that needed to be written could not be found in the
	blob lookup table of `wim.`  See `G_nonstandalone_wims` (upstream header section).
**Error `WIMLIB_ERR_WRITE`:**
	An error occurred when trying to write data to the new WIM file.

This function can additionally return `WIMLIB_ERR_DECOMPRESSION`,
`WIMLIB_ERR_INVALID_METADATA_RESOURCE`, `WIMLIB_ERR_METADATA_NOT_FOUND`,
`WIMLIB_ERR_READ`, or `WIMLIB_ERR_UNEXPECTED_END_OF_FILE`, all of which
indicate failure (for different reasons) to read the data from a WIM file.

If a progress function is registered with `wim,` then it will receive the
messages `WIMLIB_PROGRESS_MSG_WRITE_STREAMS`,
`WIMLIB_PROGRESS_MSG_WRITE_METADATA_BEGIN`, and
`WIMLIB_PROGRESS_MSG_WRITE_METADATA_END`.

### `wimlib_write_to_fd`

Source: [include/wimlib.h:4639](/tmp/wimlib/include/wimlib.h:4639).

```c
int
wimlib_write_to_fd(WIMStruct *wim,
		   int fd,
		   int image,
		   int write_flags,
		   unsigned num_threads);
```

Same as wimlib_write(), but write the WIM directly to a file descriptor,
which need not be seekable if the write is done in a special pipable WIM
format by providing `WIMLIB_WRITE_FLAG_PIPABLE` in `write_flags.`  This can,
for example, allow capturing a WIM image and streaming it over the network.
See `subsec_pipable_wims` (upstream header section) for more information about pipable WIMs.

The file descriptor `fd` will `not` be closed when the write is complete;
the calling code is responsible for this.

**Return:** 0 on success; a `wimlib_error_code` value on failure.  The possible
error codes include those that can be returned by wimlib_write() as well as
the following:

**Error `WIMLIB_ERR_INVALID_PARAM`:**
	`fd` was not seekable, but `WIMLIB_WRITE_FLAG_PIPABLE` was not
	specified in `write_flags.`

## External resources and incremental templates

### `wimlib_reference_resource_files`

Source: [include/wimlib.h:3894](/tmp/wimlib/include/wimlib.h:3894).

```c
int
wimlib_reference_resource_files(WIMStruct *wim,
				const wimlib_tchar * const *resource_wimfiles_or_globs,
				unsigned count,
				int ref_flags,
				int open_flags);
```

Reference file data from other WIM files or split WIM parts.  This function
can be used on WIMs that are not standalone, such as split or "delta" WIMs,
to load additional file data before calling a function such as
wimlib_extract_image() that requires the file data to be present.

**Parameter `wim`:**
	The `WIMStruct` for a WIM that contains metadata resources, but is not
	necessarily "standalone".  In the case of split WIMs, this should be the
	first part, since only the first part contains the metadata resources.
	In the case of delta WIMs, this should be the delta WIM rather than the
	WIM on which it is based.
**Parameter `resource_wimfiles_or_globs`:**
	Array of paths to WIM files and/or split WIM parts to reference.
	Alternatively, when `WIMLIB_REF_FLAG_GLOB_ENABLE` is specified in `ref_flags,` these are treated as globs rather than literal paths.  That
	is, using this function you can specify zero or more globs, each of
	which expands to one or more literal paths.
**Parameter `count`:**
	Number of entries in `resource_wimfiles_or_globs.`
**Parameter `ref_flags`:**
	Bitwise OR of `WIMLIB_REF_FLAG_GLOB_ENABLE` and/or
	`WIMLIB_REF_FLAG_GLOB_ERR_ON_NOMATCH`.
**Parameter `open_flags`:**
	Additional open flags, such as `WIMLIB_OPEN_FLAG_CHECK_INTEGRITY`, to
	pass to internal calls to wimlib_open_wim() on the reference files.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_GLOB_HAD_NO_MATCHES`:**
	One of the specified globs did not match any paths (only with both
	`WIMLIB_REF_FLAG_GLOB_ENABLE` and `WIMLIB_REF_FLAG_GLOB_ERR_ON_NOMATCH`
	specified in `ref_flags).`
**Error `WIMLIB_ERR_READ`:**
	I/O or permissions error while processing a file glob.

This function can additionally return most values that can be returned by
wimlib_open_wim().

### `wimlib_reference_resources`

Source: [include/wimlib.h:3921](/tmp/wimlib/include/wimlib.h:3921).

```c
int
wimlib_reference_resources(WIMStruct *wim, WIMStruct **resource_wims,
			   unsigned num_resource_wims, int ref_flags);
```

Similar to wimlib_reference_resource_files(), but operates at a lower level
where the caller must open the `WIMStruct` for each referenced file itself.

**Parameter `wim`:**
	The `WIMStruct` for a WIM that contains metadata resources, but is not
	necessarily "standalone".  In the case of split WIMs, this should be the
	first part, since only the first part contains the metadata resources.
**Parameter `resource_wims`:**
	Array of pointers to the `WIMStruct`'s for additional resource WIMs or
	split WIM parts to reference.
**Parameter `num_resource_wims`:**
	Number of entries in `resource_wims.`
**Parameter `ref_flags`:**
	Reserved; must be 0.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

### `wimlib_reference_template_image`

Source: [include/wimlib.h:3987](/tmp/wimlib/include/wimlib.h:3987).

```c
int
wimlib_reference_template_image(WIMStruct *wim, int new_image,
				WIMStruct *template_wim, int template_image,
				int flags);
```

Declare that a newly added image is mostly the same as a prior image, but
captured at a later point in time, possibly with some modifications in the
intervening time.  This is designed to be used in incremental backups of the
same filesystem or directory tree.

This function compares the metadata of the directory tree of the newly added
image against that of the old image.  Any files that are present in both the
newly added image and the old image and have timestamps that indicate they
haven't been modified are deemed not to have been modified and have their
checksums copied from the old image.  Because of this and because WIM uses
single-instance streams, such files need not be read from the filesystem when
the WIM is being written or overwritten.  Note that these unchanged files
will still be "archived" and will be logically present in the new image; the
optimization is that they don't need to actually be read from the filesystem
because the WIM already contains them.

This function is provided to optimize incremental backups.  The resulting WIM
file will still be the same regardless of whether this function is called.
(This is, however, assuming that timestamps have not been manipulated or
unmaintained as to trick this function into thinking a file has not been
modified when really it has.  To partly guard against such cases, other
metadata such as file sizes will be checked as well.)

This function must be called after adding the new image (e.g. with
wimlib_add_image()), but before writing the updated WIM file (e.g. with
wimlib_overwrite()).

**Parameter `wim`:**
	Pointer to the `WIMStruct` containing the newly added image.
**Parameter `new_image`:**
	The 1-based index in `wim` of the newly added image.
**Parameter `template_wim`:**
	Pointer to the `WIMStruct` containing the template image.  This can be,
	but does not have to be, the same `WIMStruct` as `wim.`
**Parameter `template_image`:**
	The 1-based index in `template_wim` of the template image.
**Parameter `flags`:**
	Reserved; must be 0.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_INVALID_IMAGE`:**
	`new_image` does not exist in `wim` or `template_image` does not
	exist in `template_wim.`
**Error `WIMLIB_ERR_METADATA_NOT_FOUND`:**
	At least one of `wim` and `template_wim` does not contain image
	metadata; for example, one of them represents a non-first part of a
	split WIM.
**Error `WIMLIB_ERR_INVALID_PARAM`:**
	Identical values were provided for the template and new image; or `new_image` specified an image that had not been modified since opening
	the WIM.

This function can additionally return `WIMLIB_ERR_DECOMPRESSION`,
`WIMLIB_ERR_INVALID_METADATA_RESOURCE`, `WIMLIB_ERR_METADATA_NOT_FOUND`,
`WIMLIB_ERR_READ`, or `WIMLIB_ERR_UNEXPECTED_END_OF_FILE`, all of which
indicate failure (for different reasons) to read the metadata resource for
the template image.

## Mount and unmount

### `wimlib_mount_image`

Source: [include/wimlib.h:3661](/tmp/wimlib/include/wimlib.h:3661).

```c
int
wimlib_mount_image(WIMStruct *wim,
		   int image,
		   const wimlib_tchar *dir,
		   int mount_flags,
		   const wimlib_tchar *staging_dir);
```

Mount an image from a WIM file on a directory read-only or read-write.

**Parameter `wim`:**
	Pointer to the `WIMStruct` containing the image to be mounted.  This
	`WIMStruct` must have a backing file.
**Parameter `image`:**
	The 1-based index of the image to mount.  This image cannot have been
	previously modified in memory.
**Parameter `dir`:**
	The path to an existing empty directory on which to mount the image.
**Parameter `mount_flags`:**
	Bitwise OR of flags prefixed with WIMLIB_MOUNT_FLAG.  Use
	`WIMLIB_MOUNT_FLAG_READWRITE` to request a read-write mount instead of a
	read-only mount.
**Parameter `staging_dir`:**
	If non-NULL, the name of a directory in which a temporary directory for
	storing modified or added files will be created.  Ignored if
	`WIMLIB_MOUNT_FLAG_READWRITE` is not specified in `mount_flags.`  If
	left `NULL,` the staging directory is created in the same directory as
	the backing WIM file.  The staging directory is automatically deleted
	when the image is unmounted.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_ALREADY_LOCKED`:**
	Another process is currently modifying the WIM file.
**Error `WIMLIB_ERR_FUSE`:**
	A non-zero status code was returned by `fuse_main().`
**Error `WIMLIB_ERR_IMAGE_HAS_MULTIPLE_REFERENCES`:**
	There are currently multiple references to the image as a result of a
	call to wimlib_export_image().  Free one before attempting the
	read-write mount.
**Error `WIMLIB_ERR_INVALID_IMAGE`:**
	`image` does not exist in `wim.`
**Error `WIMLIB_ERR_INVALID_PARAM`:**
	`wim` was `NULL;` or `dir` was NULL or an empty string; or an
	unrecognized flag was specified in `mount_flags;` or the image has
	already been modified in memory (e.g. by wimlib_update_image()).
**Error `WIMLIB_ERR_MKDIR`:**
	`WIMLIB_MOUNT_FLAG_READWRITE` was specified in `mount_flags,` but the
	staging directory could not be created.
**Error `WIMLIB_ERR_WIM_IS_READONLY`:**
	`WIMLIB_MOUNT_FLAG_READWRITE` was specified in `mount_flags,` but the
	WIM file is considered read-only because of any of the reasons mentioned
	in the documentation for the `WIMLIB_OPEN_FLAG_WRITE_ACCESS` flag.
**Error `WIMLIB_ERR_UNSUPPORTED`:**
	Mounting is not supported in this build of the library.

This function can additionally return `WIMLIB_ERR_DECOMPRESSION`,
`WIMLIB_ERR_INVALID_METADATA_RESOURCE`, `WIMLIB_ERR_METADATA_NOT_FOUND`,
`WIMLIB_ERR_READ`, or `WIMLIB_ERR_UNEXPECTED_END_OF_FILE`, all of which
indicate failure (for different reasons) to read the metadata resource for
the image to mount.

The ability to mount WIM images is implemented using FUSE (Filesystem in
UserSpacE).  Depending on how FUSE is set up on your system, this function
may work as normal users in addition to the root user.

Mounting WIM images is not supported if wimlib was configured
`--without-fuse`.  This includes Windows builds of wimlib;
`WIMLIB_ERR_UNSUPPORTED` will be returned in such cases.

Calling this function daemonizes the process, unless
`WIMLIB_MOUNT_FLAG_DEBUG` was specified or an early error occurs.

It is safe to mount multiple images from the same WIM file read-only at the
same time, but only if different `WIMStruct`'s are used.  It is `not` safe
to mount multiple images from the same WIM file read-write at the same time.

To unmount the image, call wimlib_unmount_image().  This may be done in a
different process.

### `wimlib_unmount_image`

Source: [include/wimlib.h:4437](/tmp/wimlib/include/wimlib.h:4437).

```c
int
wimlib_unmount_image(const wimlib_tchar *dir, int unmount_flags);
```

Unmount a WIM image that was mounted using wimlib_mount_image().

When unmounting a read-write mounted image, the default behavior is to
discard changes to the image.  Use `WIMLIB_UNMOUNT_FLAG_COMMIT` to cause the
image to be committed.

**Parameter `dir`:**
	The directory on which the WIM image is mounted.
**Parameter `unmount_flags`:**
	Bitwise OR of flags prefixed with `WIMLIB_UNMOUNT_FLAG.`

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_NOT_A_MOUNTPOINT`:**
	There is no WIM image mounted on the specified directory.
**Error `WIMLIB_ERR_MOUNTED_IMAGE_IS_BUSY`:**
	The read-write mounted image cannot be committed because there are file
	descriptors open to it, and `WIMLIB_UNMOUNT_FLAG_FORCE` was not
	specified.
**Error `WIMLIB_ERR_MQUEUE`:**
	Could not create a POSIX message queue.
**Error `WIMLIB_ERR_NOT_PERMITTED_TO_UNMOUNT`:**
	The image was mounted by a different user.
**Error `WIMLIB_ERR_UNSUPPORTED`:**
	Mounting is not supported in this build of the library.

Note: you can also unmount the image by using the `umount()` system call, or
by using the `umount` or `fusermount` programs.  However, you need to call
this function if you want changes to be committed.

### `wimlib_unmount_image_with_progress`

Source: [include/wimlib.h:4448](/tmp/wimlib/include/wimlib.h:4448).

```c
int
wimlib_unmount_image_with_progress(const wimlib_tchar *dir,
				   int unmount_flags,
				   wimlib_progress_func_t progfunc,
				   void *progctx);
```

Same as wimlib_unmount_image(), but allows specifying a progress function.
The progress function will receive a `WIMLIB_PROGRESS_MSG_UNMOUNT_BEGIN`
message.  In addition, if changes are committed from a read-write mount, the
progress function will receive `WIMLIB_PROGRESS_MSG_WRITE_STREAMS` messages.

## Compression and decompression

### `wimlib_set_default_compression_level`

Source: [include/wimlib.h:4697](/tmp/wimlib/include/wimlib.h:4697).

```c
int
wimlib_set_default_compression_level(int ctype, unsigned int compression_level);
```

Set the default compression level for the specified compression type.  This
is the compression level that wimlib_create_compressor() assumes if it is
called with `compression_level` specified as 0.

wimlib's WIM writing code (e.g. wimlib_write()) will pass 0 to
wimlib_create_compressor() internally.  Therefore, calling this function will
affect the compression level of any data later written to WIM files using the
specified compression type.

The initial state, before this function is called, is that all compression
types have a default compression level of 50.

**Parameter `ctype`:**
	Compression type for which to set the default compression level, as one
	of the `wimlib_compression_type` constants.  Or, if this is the special
	value -1, the default compression levels for all compression types will
	be set.
**Parameter `compression_level`:**
	The default compression level to set.  If 0, the "default default" level
	of 50 is restored.  Otherwise, a higher value indicates higher
	compression, whereas a lower value indicates lower compression.  See
	wimlib_create_compressor() for more information.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_INVALID_COMPRESSION_TYPE`:**
	`ctype` was neither a supported compression type nor -1.

### `wimlib_get_compressor_needed_memory`

Source: [include/wimlib.h:4708](/tmp/wimlib/include/wimlib.h:4708).

```c
uint64_t
wimlib_get_compressor_needed_memory(enum wimlib_compression_type ctype,
				    size_t max_block_size,
				    unsigned int compression_level);
```

Return the approximate number of bytes needed to allocate a compressor with
wimlib_create_compressor() for the specified compression type, maximum block
size, and compression level.  `compression_level` may be 0, in which case
the current default compression level for `ctype` is used.  Returns 0 if the
compression type is invalid, or the `max_block_size` for that compression
type is invalid.

### `wimlib_create_compressor`

Source: [include/wimlib.h:4783](/tmp/wimlib/include/wimlib.h:4783).

```c
int
wimlib_create_compressor(enum wimlib_compression_type ctype,
			 size_t max_block_size,
			 unsigned int compression_level,
			 struct wimlib_compressor **compressor_ret);
```

Allocate a compressor for the specified compression type using the specified
parameters.  This function is part of wimlib's compression API; it is not
necessary to call this to process a WIM file.

**Parameter `ctype`:**
	Compression type for which to create the compressor, as one of the
	`wimlib_compression_type` constants.
**Parameter `max_block_size`:**
	The maximum compression block size to support.  This specifies the
	maximum allowed value for the `uncompressed_size` parameter of
	wimlib_compress() when called using this compressor.
	<br/>
	Usually, the amount of memory used by the compressor will scale in
	proportion to the `max_block_size` parameter.
	wimlib_get_compressor_needed_memory() can be used to query the specific
	amount of memory that will be required.
	<br/>
	This parameter must be at least 1 and must be less than or equal to a
	compression-type-specific limit.
	<br/>
	In general, the same value of `max_block_size` must be passed to
	wimlib_create_decompressor() when the data is later decompressed.
	However, some compression types have looser requirements regarding this.
**Parameter `compression_level`:**
	The compression level to use.  If 0, the default compression level (50,
	or another value as set through wimlib_set_default_compression_level())
	is used.  Otherwise, a higher value indicates higher compression.  The
	values are scaled so that 10 is low compression, 50 is medium
	compression, and 100 is high compression.  This is not a percentage;
	values above 100 are also valid.
	<br/>
	Using a higher-than-default compression level can result in a better
	compression ratio, but can significantly reduce performance.  Similarly,
	using a lower-than-default compression level can result in better
	performance, but can significantly worsen the compression ratio.  The
	exact results will depend heavily on the compression type and what
	algorithms are implemented for it.  If you are considering using a
	non-default compression level, you should run benchmarks to see if it is
	worthwhile for your application.
	<br/>
	The compression level does not affect the format of the compressed data.
	Therefore, it is a compressor-only parameter and does not need to be
	passed to the decompressor.
	<br/>
	Since wimlib v1.8.0, this parameter can be OR-ed with the flag
	`WIMLIB_COMPRESSOR_FLAG_DESTRUCTIVE`.  This creates the compressor in a
	mode where it is allowed to modify the input buffer.  Specifically, in
	this mode, if compression succeeds, the input buffer may have been
	modified, whereas if compression does not succeed the input buffer still
	may have been written to but will have been restored exactly to its
	original state.  This mode is designed to save some memory when using
	large buffer sizes.
**Parameter `compressor_ret`:**
	A location into which to return the pointer to the allocated compressor.
	The allocated compressor can be used for any number of calls to
	wimlib_compress() before being freed with wimlib_free_compressor().

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_INVALID_COMPRESSION_TYPE`:**
	`ctype` was not a supported compression type.
**Error `WIMLIB_ERR_INVALID_PARAM`:**
	`max_block_size` was invalid for the compression type, or `compressor_ret` was `NULL.`
**Error `WIMLIB_ERR_NOMEM`:**
	Insufficient memory to allocate the compressor.

### `wimlib_compress`

Source: [include/wimlib.h:4809](/tmp/wimlib/include/wimlib.h:4809).

```c
size_t
wimlib_compress(const void *uncompressed_data, size_t uncompressed_size,
		void *compressed_data, size_t compressed_size_avail,
		struct wimlib_compressor *compressor);
```

Compress a buffer of data.

**Parameter `uncompressed_data`:**
	Buffer containing the data to compress.
**Parameter `uncompressed_size`:**
	Size, in bytes, of the data to compress.  This cannot be greater than
	the `max_block_size` with which wimlib_create_compressor() was called.
	(If it is, the data will not be compressed and 0 will be returned.)
**Parameter `compressed_data`:**
	Buffer into which to write the compressed data.
**Parameter `compressed_size_avail`:**
	Number of bytes available in `compressed_data.`
**Parameter `compressor`:**
	A compressor previously allocated with wimlib_create_compressor().

**Return:**
	The size of the compressed data, in bytes, or 0 if the data could not be
	compressed to `compressed_size_avail` or fewer bytes.

### `wimlib_free_compressor`

Source: [include/wimlib.h:4820](/tmp/wimlib/include/wimlib.h:4820).

```c
void
wimlib_free_compressor(struct wimlib_compressor *compressor);
```

Free a compressor previously allocated with wimlib_create_compressor().

**Parameter `compressor`:**
	The compressor to free.  If `NULL,` no action is taken.

### `wimlib_create_decompressor`

Source: [include/wimlib.h:4856](/tmp/wimlib/include/wimlib.h:4856).

```c
int
wimlib_create_decompressor(enum wimlib_compression_type ctype,
			   size_t max_block_size,
			   struct wimlib_decompressor **decompressor_ret);
```

Allocate a decompressor for the specified compression type.  This function is
part of wimlib's compression API; it is not necessary to call this to process
a WIM file.

**Parameter `ctype`:**
	Compression type for which to create the decompressor, as one of the
	`wimlib_compression_type` constants.
**Parameter `max_block_size`:**
	The maximum compression block size to support.  This specifies the
	maximum allowed value for the `uncompressed_size` parameter of
	wimlib_decompress().
	<br/>
	In general, this parameter must be the same as the `max_block_size`
	that was passed to wimlib_create_compressor() when the data was
	compressed.  However, some compression types have looser requirements
	regarding this.
**Parameter `decompressor_ret`:**
	A location into which to return the pointer to the allocated
	decompressor.  The allocated decompressor can be used for any number of
	calls to wimlib_decompress() before being freed with
	wimlib_free_decompressor().

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_INVALID_COMPRESSION_TYPE`:**
	`ctype` was not a supported compression type.
**Error `WIMLIB_ERR_INVALID_PARAM`:**
	`max_block_size` was invalid for the compression type, or `decompressor_ret` was `NULL.`
**Error `WIMLIB_ERR_NOMEM`:**
	Insufficient memory to allocate the decompressor.

### `wimlib_decompress`

Source: [include/wimlib.h:4888](/tmp/wimlib/include/wimlib.h:4888).

```c
int
wimlib_decompress(const void *compressed_data, size_t compressed_size,
		  void *uncompressed_data, size_t uncompressed_size,
		  struct wimlib_decompressor *decompressor);
```

Decompress a buffer of data.

**Parameter `compressed_data`:**
	Buffer containing the data to decompress.
**Parameter `compressed_size`:**
	Size, in bytes, of the data to decompress.
**Parameter `uncompressed_data`:**
	Buffer into which to write the uncompressed data.
**Parameter `uncompressed_size`:**
	Size, in bytes, of the data when uncompressed.  This cannot exceed the
	`max_block_size` with which wimlib_create_decompressor() was called.
	(If it does, the data will not be decompressed and a nonzero value will
	be returned.)
**Parameter `decompressor`:**
	A decompressor previously allocated with wimlib_create_decompressor().

**Return:** 0 on success; nonzero on failure.

No specific error codes are defined; any nonzero value indicates that the
decompression failed.  This can only occur if the data is truly invalid;
there will never be transient errors like "out of memory", for example.

This function requires that the exact uncompressed size of the data be passed
as the `uncompressed_size` parameter.  If this is not done correctly,
decompression may fail or the data may be decompressed incorrectly.

### `wimlib_free_decompressor`

Source: [include/wimlib.h:4899](/tmp/wimlib/include/wimlib.h:4899).

```c
void
wimlib_free_decompressor(struct wimlib_decompressor *decompressor);
```

Free a decompressor previously allocated with wimlib_create_decompressor().

**Parameter `decompressor`:**
	The decompressor to free.  If `NULL,` no action is taken.

## Callbacks, diagnostics, allocators and text

### `wimlib_load_text_file`

Source: [include/wimlib.h:3582](/tmp/wimlib/include/wimlib.h:3582).

```c
int
wimlib_load_text_file(const wimlib_tchar *path,
		      wimlib_tchar **tstr_ret, size_t *tstr_nchars_ret);
```

Load a UTF-8 or UTF-16LE encoded text file into memory.

**Parameter `path`:**
	The path to the file, or NULL or "-" to use standard input.
**Parameter `tstr_ret`:**
	On success, a buffer containing the file's text as a "wimlib_tchar"
	string is returned here.  The buffer must be freed using free().
**Parameter `tstr_nchars_ret`:**
	On success, the length of the text in "wimlib_tchar"s is returned here.

**Return:** 0 on success; a `wimlib_error_code` value on failure.

### `wimlib_register_progress_function`

Source: [include/wimlib.h:4007](/tmp/wimlib/include/wimlib.h:4007).

```c
void
wimlib_register_progress_function(WIMStruct *wim,
				  wimlib_progress_func_t progfunc,
				  void *progctx);
```

Register a progress function with a `WIMStruct`.

**Parameter `wim`:**
	The `WIMStruct` for which to register the progress function.
**Parameter `progfunc`:**
	Pointer to the progress function to register.  If the WIM already has a
	progress function registered, it will be replaced with this one.  If `NULL,` the current progress function (if any) will be unregistered.
**Parameter `progctx`:**
	The value which will be passed as the third argument to calls to `progfunc.`

### `wimlib_set_error_file`

Source: [include/wimlib.h:4073](/tmp/wimlib/include/wimlib.h:4073).

```c
int
wimlib_set_error_file(FILE *fp);
```

Set the file to which the library will print error and warning messages.

This version of the function takes a C library `FILE*` opened for
writing (or appending).  Use wimlib_set_error_file_by_name() to specify the
file by name instead.

This also enables error messages, as if by a call to
wimlib_set_print_errors(true).

**Return:** 0

### `wimlib_set_error_file_by_name`

Source: [include/wimlib.h:4090](/tmp/wimlib/include/wimlib.h:4090).

```c
int
wimlib_set_error_file_by_name(const wimlib_tchar *path);
```

Set the path to the file to which the library will print error and warning
messages.  The library will open this file for appending.

This also enables error messages, as if by a call to
wimlib_set_print_errors(true).

**Return:** 0 on success; a `wimlib_error_code` value on failure.

**Error `WIMLIB_ERR_OPEN`:**
	The file named by `path` could not be opened for appending.

### `wimlib_set_memory_allocator`

Source: [include/wimlib.h:4194](/tmp/wimlib/include/wimlib.h:4194).

```c
int
wimlib_set_memory_allocator(void *(*malloc_func)(size_t),
			    void (*free_func)(void *),
			    void *(*realloc_func)(void *, size_t));
```

Set the functions that wimlib uses to allocate and free memory.

These settings are global and not per-WIM.

The default is to use the default `malloc(),` `free(),` and `realloc()`
from the standard C library.

Note: some external functions, such as those in `libntfs-3g,` may use the
standard memory allocation functions regardless of this setting.

**Parameter `malloc_func`:**
	A function equivalent to `malloc()` that wimlib will use to allocate
	memory.  If `NULL,` the allocator function is set back to the default
	`malloc()` from the C library.
**Parameter `free_func`:**
	A function equivalent to `free()` that wimlib will use to free memory.
	If `NULL,` the free function is set back to the default `free()` from
	the C library.
**Parameter `realloc_func`:**
	A function equivalent to `realloc()` that wimlib will use to reallocate
	memory.  If `NULL,` the free function is set back to the default `realloc()` from the C library.

**Return:** 0

### `wimlib_set_print_errors`

Source: [include/wimlib.h:4288](/tmp/wimlib/include/wimlib.h:4288).

```c
int
wimlib_set_print_errors(bool show_messages);
```

Set whether wimlib can print error and warning messages to the error file,
which defaults to standard error.  Error and warning messages may provide
information that cannot be determined only from returned error codes.

By default, error messages are not printed.

This setting applies globally (it is not per-WIM).

This can be called before wimlib_global_init().

**Parameter `show_messages`:**
	`true` if messages are to be printed; `false` if messages are not to
	be printed.

**Return:** 0

## Annotated public types, callbacks, constants, flags and error codes

The following source excerpt retains comments because constraints on individual
fields and masks are part of the API. Doxygen group markers are editorial only.

```c
#define WIMLIB_MAJOR_VERSION 1

/** Minor version of the library (for example, the 2 in 1.2.5). */
#define WIMLIB_MINOR_VERSION 14

/** Patch version of the library (for example, the 5 in 1.2.5). */
#define WIMLIB_PATCH_VERSION 5

#ifdef __cplusplus
extern "C" {
#endif

/*
 * To represent file timestamps, wimlib's API originally used the POSIX 'struct
 * timespec'.  This was a mistake because when building wimlib for 32-bit
 * Windows with MinGW we ended up originally using 32-bit time_t which isn't
 * year 2038-safe, and therefore we had to later add fields like
 * 'creation_time_high' to hold the high 32 bits of each timestamp.  Moreover,
 * old Visual Studio versions did not define struct timespec, while newer ones
 * define it but with 64-bit tv_sec.  So to at least avoid a missing or
 * incompatible 'struct timespec' definition, define the correct struct
 * ourselves when this header is included on Windows.
 */
#ifdef _WIN32
struct wimlib_timespec {
	/* Seconds since start of UNIX epoch (January 1, 1970) */
#ifdef _WIN64
	int64_t tv_sec;
#else
	int32_t tv_sec;
#endif
	/* Nanoseconds (0-999999999) */
	int32_t tv_nsec;
};
#else
#  define wimlib_timespec  timespec  /* standard definition */
#endif

/**
 * Opaque structure that represents a WIM, possibly backed by an on-disk file.
 * See @ref sec_basic_wim_handling_concepts for more information.
 */
#ifndef WIMLIB_WIMSTRUCT_DECLARED
typedef struct WIMStruct WIMStruct;
#define WIMLIB_WIMSTRUCT_DECLARED
#endif

#ifdef _WIN32
typedef wchar_t wimlib_tchar;
#else
/** See @ref sec_encodings */
typedef char wimlib_tchar;
#endif

#ifdef _WIN32
/** Path separator for WIM paths passed back to progress callbacks.
 * This is forward slash on UNIX and backslash on Windows.  */
#  define WIMLIB_WIM_PATH_SEPARATOR '\\'
#  define WIMLIB_WIM_PATH_SEPARATOR_STRING L"\\"
#else
/** Path separator for WIM paths passed back to progress callbacks.
 * This is forward slash on UNIX and backslash on Windows.  */
#  define WIMLIB_WIM_PATH_SEPARATOR '/'
#  define WIMLIB_WIM_PATH_SEPARATOR_STRING "/"
#endif

/** A string containing a single path separator; use this to specify the root
 * directory of a WIM image.  */
#define WIMLIB_WIM_ROOT_PATH WIMLIB_WIM_PATH_SEPARATOR_STRING

/** Use this to test if the specified path refers to the root directory of the
 * WIM image.  */
#define WIMLIB_IS_WIM_ROOT_PATH(path) \
		((path)[0] == WIMLIB_WIM_PATH_SEPARATOR &&	\
		 (path)[1] == 0)

/** Length of a Globally Unique Identifier (GUID), in bytes.  */
#define WIMLIB_GUID_LEN 16

/**
 * Specifies a compression type.
 *
 * A WIM file has a default compression type, indicated by its file header.
 * Normally, each resource in the WIM file is compressed with this compression
 * type.  However, resources may be stored as uncompressed; for example, wimlib
 * may do so if a resource does not compress to less than its original size.  In
 * addition, a WIM with the new version number of 3584, or "ESD file", might
 * contain solid resources with different compression types.
 */
enum wimlib_compression_type {
	/**
	 * No compression.
	 *
	 * This is a valid argument to wimlib_create_new_wim() and
	 * wimlib_set_output_compression_type(), but not to the functions in the
	 * compression API such as wimlib_create_compressor().
	 */
	WIMLIB_COMPRESSION_TYPE_NONE = 0,

	/**
	 * The XPRESS compression format.  This format combines Lempel-Ziv
	 * factorization with Huffman encoding.  Compression and decompression
	 * are both fast.  This format supports chunk sizes that are powers of 2
	 * between <c>2^12</c> and <c>2^16</c>, inclusively.
	 *
	 * wimlib's XPRESS compressor will, with the default settings, usually
	 * produce a better compression ratio, and work more quickly, than the
	 * implementation in Microsoft's WIMGAPI (as of Windows 8.1).
	 * Non-default compression levels are also supported.  For example,
	 * level 80 will enable two-pass optimal parsing, which is significantly
	 * slower but usually improves compression by several percent over the
	 * default level of 50.
	 *
	 * If using wimlib_create_compressor() to create an XPRESS compressor
	 * directly, the @p max_block_size parameter may be any positive value
	 * up to and including <c>2^16</c>.
	 */
	WIMLIB_COMPRESSION_TYPE_XPRESS = 1,

	/**
	 * The LZX compression format.  This format combines Lempel-Ziv
	 * factorization with Huffman encoding, but with more features and
	 * complexity than XPRESS.  Compression is slow to somewhat fast,
	 * depending on the settings.  Decompression is fast but slower than
	 * XPRESS.  This format supports chunk sizes that are powers of 2
	 * between <c>2^15</c> and <c>2^21</c>, inclusively.  Note: chunk sizes
	 * other than <c>2^15</c> are not compatible with the Microsoft
	 * implementation.
	 *
	 * wimlib's LZX compressor will, with the default settings, usually
	 * produce a better compression ratio, and work more quickly, than the
	 * implementation in Microsoft's WIMGAPI (as of Windows 8.1).
	 * Non-default compression levels are also supported.  For example,
	 * level 20 will provide fast compression, almost as fast as XPRESS.
	 *
	 * If using wimlib_create_compressor() to create an LZX compressor
	 * directly, the @p max_block_size parameter may be any positive value
	 * up to and including <c>2^21</c>.
	 */
	WIMLIB_COMPRESSION_TYPE_LZX = 2,

	/**
	 * The LZMS compression format.  This format combines Lempel-Ziv
	 * factorization with adaptive Huffman encoding and range coding.
	 * Compression and decompression are both fairly slow.  This format
	 * supports chunk sizes that are powers of 2 between <c>2^15</c> and
	 * <c>2^30</c>, inclusively.  This format is best used for large chunk
	 * sizes.  Note: LZMS compression is only compatible with wimlib v1.6.0
	 * and later, WIMGAPI Windows 8 and later, and DISM Windows 8.1 and
	 * later.  Also, chunk sizes larger than <c>2^26</c> are not compatible
	 * with the Microsoft implementation.
	 *
	 * wimlib's LZMS compressor will, with the default settings, usually
	 * produce a better compression ratio, and work more quickly, than the
	 * implementation in Microsoft's WIMGAPI (as of Windows 8.1).  There is
	 * limited support for non-default compression levels, but compression
	 * will be noticeably faster if you choose a level < 35.
	 *
	 * If using wimlib_create_compressor() to create an LZMS compressor
	 * directly, the @p max_block_size parameter may be any positive value
	 * up to and including <c>2^30</c>.
	 */
	WIMLIB_COMPRESSION_TYPE_LZMS = 3,
};

/** @} */
/** @addtogroup G_progress
 * @{ */

/** Possible values of the first parameter to the user-supplied
 * ::wimlib_progress_func_t progress function */
enum wimlib_progress_msg {

	/** A WIM image is about to be extracted.  @p info will point to
	 * ::wimlib_progress_info.extract.  This message is received once per
	 * image for calls to wimlib_extract_image() and
	 * wimlib_extract_image_from_pipe().  */
	WIMLIB_PROGRESS_MSG_EXTRACT_IMAGE_BEGIN = 0,

	/** One or more file or directory trees within a WIM image is about to
	 * be extracted.  @p info will point to ::wimlib_progress_info.extract.
	 * This message is received only once per wimlib_extract_paths() and
	 * wimlib_extract_pathlist(), since wimlib combines all paths into a
	 * single extraction operation for optimization purposes.  */
	WIMLIB_PROGRESS_MSG_EXTRACT_TREE_BEGIN = 1,

	/** This message may be sent periodically (not for every file) while
	 * files and directories are being created, prior to file data
	 * extraction.  @p info will point to ::wimlib_progress_info.extract.
	 * In particular, the @p current_file_count and @p end_file_count
	 * members may be used to track the progress of this phase of
	 * extraction.  */
	WIMLIB_PROGRESS_MSG_EXTRACT_FILE_STRUCTURE = 3,

	/** File data is currently being extracted.  @p info will point to
	 * ::wimlib_progress_info.extract.  This is the main message to track
	 * the progress of an extraction operation.  */
	WIMLIB_PROGRESS_MSG_EXTRACT_STREAMS = 4,

	/** Starting to read a new part of a split pipable WIM over the pipe.
	 * @p info will point to ::wimlib_progress_info.extract.  */
	WIMLIB_PROGRESS_MSG_EXTRACT_SPWM_PART_BEGIN = 5,

	/** This message may be sent periodically (not necessarily for every
	 * file) while file and directory metadata is being extracted, following
	 * file data extraction.  @p info will point to
	 * ::wimlib_progress_info.extract.  The @p current_file_count and @p
	 * end_file_count members may be used to track the progress of this
	 * phase of extraction.  */
	WIMLIB_PROGRESS_MSG_EXTRACT_METADATA = 6,

	/** The image has been successfully extracted.  @p info will point to
	 * ::wimlib_progress_info.extract.  This is paired with
	 * ::WIMLIB_PROGRESS_MSG_EXTRACT_IMAGE_BEGIN.  */
	WIMLIB_PROGRESS_MSG_EXTRACT_IMAGE_END = 7,

	/** The files or directory trees have been successfully extracted.  @p
	 * info will point to ::wimlib_progress_info.extract.  This is paired
	 * with ::WIMLIB_PROGRESS_MSG_EXTRACT_TREE_BEGIN.  */
	WIMLIB_PROGRESS_MSG_EXTRACT_TREE_END = 8,

	/** The directory or NTFS volume is about to be scanned for metadata.
	 * @p info will point to ::wimlib_progress_info.scan.  This message is
	 * received once per call to wimlib_add_image(), or once per capture
	 * source passed to wimlib_add_image_multisource(), or once per add
	 * command passed to wimlib_update_image().  */
	WIMLIB_PROGRESS_MSG_SCAN_BEGIN = 9,

	/** A directory or file has been scanned.  @p info will point to
	 * ::wimlib_progress_info.scan, and its @p cur_path member will be
	 * valid.  This message is only sent if ::WIMLIB_ADD_FLAG_VERBOSE has
	 * been specified.  */
	WIMLIB_PROGRESS_MSG_SCAN_DENTRY = 10,

	/** The directory or NTFS volume has been successfully scanned.  @p info
	 * will point to ::wimlib_progress_info.scan.  This is paired with a
	 * previous ::WIMLIB_PROGRESS_MSG_SCAN_BEGIN message, possibly with many
	 * intervening ::WIMLIB_PROGRESS_MSG_SCAN_DENTRY messages.  */
	WIMLIB_PROGRESS_MSG_SCAN_END = 11,

	/** File data is currently being written to the WIM.  @p info will point
	 * to ::wimlib_progress_info.write_streams.  This message may be
	 * received many times while the WIM file is being written or appended
	 * to with wimlib_write(), wimlib_overwrite(), or wimlib_write_to_fd().
	 * Since wimlib v1.13.4 it will also be received when a split WIM part
	 * is being written by wimlib_split().  */
	WIMLIB_PROGRESS_MSG_WRITE_STREAMS = 12,

	/** Per-image metadata is about to be written to the WIM file.  @p info
	 * will not be valid. */
	WIMLIB_PROGRESS_MSG_WRITE_METADATA_BEGIN = 13,

	/** The per-image metadata has been written to the WIM file.  @p info
	 * will not be valid.  This message is paired with a preceding
	 * ::WIMLIB_PROGRESS_MSG_WRITE_METADATA_BEGIN message.  */
	WIMLIB_PROGRESS_MSG_WRITE_METADATA_END = 14,

	/** wimlib_overwrite() has successfully renamed the temporary file to
	 * the original WIM file, thereby committing the changes to the WIM
	 * file.  @p info will point to ::wimlib_progress_info.rename.  Note:
	 * this message is not received if wimlib_overwrite() chose to append to
	 * the WIM file in-place.  */
	WIMLIB_PROGRESS_MSG_RENAME = 15,

	/** The contents of the WIM file are being checked against the integrity
	 * table.  @p info will point to ::wimlib_progress_info.integrity.  This
	 * message is only received (and may be received many times) when
	 * wimlib_open_wim_with_progress() is called with the
	 * ::WIMLIB_OPEN_FLAG_CHECK_INTEGRITY flag.  */
	WIMLIB_PROGRESS_MSG_VERIFY_INTEGRITY = 16,

	/** An integrity table is being calculated for the WIM being written.
	 * @p info will point to ::wimlib_progress_info.integrity.  This message
	 * is only received (and may be received many times) when a WIM file is
	 * being written with the flag ::WIMLIB_WRITE_FLAG_CHECK_INTEGRITY.  */
	WIMLIB_PROGRESS_MSG_CALC_INTEGRITY = 17,

	/** A wimlib_split() operation is in progress, and a new split part is
	 * about to be started.  @p info will point to
	 * ::wimlib_progress_info.split.  */
	WIMLIB_PROGRESS_MSG_SPLIT_BEGIN_PART = 19,

	/** A wimlib_split() operation is in progress, and a split part has been
	 * finished. @p info will point to ::wimlib_progress_info.split.  */
	WIMLIB_PROGRESS_MSG_SPLIT_END_PART = 20,

	/** A WIM update command is about to be executed. @p info will point to
	 * ::wimlib_progress_info.update.  This message is received once per
	 * update command when wimlib_update_image() is called with the flag
	 * ::WIMLIB_UPDATE_FLAG_SEND_PROGRESS.  */
	WIMLIB_PROGRESS_MSG_UPDATE_BEGIN_COMMAND = 21,

	/** A WIM update command has been executed. @p info will point to
	 * ::wimlib_progress_info.update.  This message is received once per
	 * update command when wimlib_update_image() is called with the flag
	 * ::WIMLIB_UPDATE_FLAG_SEND_PROGRESS.  */
	WIMLIB_PROGRESS_MSG_UPDATE_END_COMMAND = 22,

	/** A file in the image is being replaced as a result of a
	 * ::wimlib_add_command without ::WIMLIB_ADD_FLAG_NO_REPLACE specified.
	 * @p info will point to ::wimlib_progress_info.replace.  This is only
	 * received when ::WIMLIB_ADD_FLAG_VERBOSE is also specified in the add
	 * command.  */
	WIMLIB_PROGRESS_MSG_REPLACE_FILE_IN_WIM = 23,

	/** An image is being extracted with ::WIMLIB_EXTRACT_FLAG_WIMBOOT, and
	 * a file is being extracted normally (not as a "WIMBoot pointer file")
	 * due to it matching a pattern in the <c>[PrepopulateList]</c> section
	 * of the configuration file
	 * <c>/Windows/System32/WimBootCompress.ini</c> in the WIM image.  @p
	 * info will point to ::wimlib_progress_info.wimboot_exclude.  */
	WIMLIB_PROGRESS_MSG_WIMBOOT_EXCLUDE = 24,

	/** Starting to unmount an image.  @p info will point to
	 * ::wimlib_progress_info.unmount.  */
	WIMLIB_PROGRESS_MSG_UNMOUNT_BEGIN = 25,

	/** wimlib has used a file's data for the last time (including all data
	 * streams, if it has multiple).  @p info will point to
	 * ::wimlib_progress_info.done_with_file.  This message is only received
	 * if ::WIMLIB_WRITE_FLAG_SEND_DONE_WITH_FILE_MESSAGES was provided.  */
	WIMLIB_PROGRESS_MSG_DONE_WITH_FILE = 26,

	/** wimlib_verify_wim() is starting to verify the metadata for an image.
	 * @p info will point to ::wimlib_progress_info.verify_image.  */
	WIMLIB_PROGRESS_MSG_BEGIN_VERIFY_IMAGE = 27,

	/** wimlib_verify_wim() has finished verifying the metadata for an
	 * image.  @p info will point to ::wimlib_progress_info.verify_image.
	 */
	WIMLIB_PROGRESS_MSG_END_VERIFY_IMAGE = 28,

	/** wimlib_verify_wim() is verifying file data integrity.  @p info will
	 * point to ::wimlib_progress_info.verify_streams.  */
	WIMLIB_PROGRESS_MSG_VERIFY_STREAMS = 29,

	/**
	 * The progress function is being asked whether a file should be
	 * excluded from capture or not.  @p info will point to
	 * ::wimlib_progress_info.test_file_exclusion.  This is a bidirectional
	 * message that allows the progress function to set a flag if the file
	 * should be excluded.
	 *
	 * This message is only received if the flag
	 * ::WIMLIB_ADD_FLAG_TEST_FILE_EXCLUSION is used.  This method for file
	 * exclusions is independent of the "capture configuration file"
	 * mechanism.
	 */
	WIMLIB_PROGRESS_MSG_TEST_FILE_EXCLUSION = 30,

	/**
	 * An error has occurred and the progress function is being asked
	 * whether to ignore the error or not.  @p info will point to
	 * ::wimlib_progress_info.handle_error.  This is a bidirectional
	 * message.
	 *
	 * This message provides a limited capability for applications to
	 * recover from "unexpected" errors (i.e. those with no in-library
	 * handling policy) arising from the underlying operating system.
	 * Normally, any such error will cause the library to abort the current
	 * operation.  By implementing a handler for this message, the
	 * application can instead choose to ignore a given error.
	 *
	 * Currently, only the following types of errors will result in this
	 * progress message being sent:
	 *
	 *	- Directory tree scan errors, e.g. from wimlib_add_image()
	 *	- Most extraction errors; currently restricted to the Windows
	 *	  build of the library only.
	 */
	WIMLIB_PROGRESS_MSG_HANDLE_ERROR = 31,
};

/** Valid return values from user-provided progress functions
 * (::wimlib_progress_func_t).
 *
 * (Note: if an invalid value is returned, ::WIMLIB_ERR_UNKNOWN_PROGRESS_STATUS
 * will be issued.)
 */
enum wimlib_progress_status {

	/** The operation should be continued.  This is the normal return value.
	 */
	WIMLIB_PROGRESS_STATUS_CONTINUE	= 0,

	/** The operation should be aborted.  This will cause the current
	 * operation to fail with ::WIMLIB_ERR_ABORTED_BY_PROGRESS.  */
	WIMLIB_PROGRESS_STATUS_ABORT	= 1,
};

/**
 * A pointer to this union is passed to the user-supplied
 * ::wimlib_progress_func_t progress function.  One (or none) of the structures
 * contained in this union will be applicable for the operation
 * (::wimlib_progress_msg) indicated in the first argument to the progress
 * function. */
union wimlib_progress_info {

	/** Valid on the message ::WIMLIB_PROGRESS_MSG_WRITE_STREAMS.  This is
	 * the primary message for tracking the progress of writing a WIM file.
	 */
	struct wimlib_progress_info_write_streams {

		/** An upper bound on the number of bytes of file data that will
		 * be written.  This number is the uncompressed size; the actual
		 * size may be lower due to compression.  In addition, this
		 * number may decrease over time as duplicated file data is
		 * discovered.  */
		uint64_t total_bytes;

		/** An upper bound on the number of distinct file data "blobs"
		 * that will be written.  This will often be similar to the
		 * "number of files", but for several reasons (hard links, named
		 * data streams, empty files, etc.) it can be different.  In
		 * addition, this number may decrease over time as duplicated
		 * file data is discovered.  */
		uint64_t total_streams;

		/** The number of bytes of file data that have been written so
		 * far.  This starts at 0 and ends at @p total_bytes.  This
		 * number is the uncompressed size; the actual size may be lower
		 * due to compression.  See @p completed_compressed_bytes for
		 * the compressed size.  */
		uint64_t completed_bytes;

		/** The number of distinct file data "blobs" that have been
		 * written so far.  This starts at 0 and ends at @p
		 * total_streams.  */
		uint64_t completed_streams;

		/** The number of threads being used for data compression; or,
		 * if no compression is being performed, this will be 1.  */
		uint32_t num_threads;

		/** The compression type being used, as one of the
		 * ::wimlib_compression_type constants.  */
		int32_t	 compression_type;

		/** The number of on-disk WIM files from which file data is
		 * being exported into the output WIM file.  This can be 0, 1,
		 * or more than 1, depending on the situation.  */
		uint32_t total_parts;

		/** This is currently broken and will always be 0.  */
		uint32_t completed_parts;

		/** Since wimlib v1.13.4: Like @p completed_bytes, but counts
		 * the compressed size.  */
		uint64_t completed_compressed_bytes;
	} write_streams;

	/** Valid on messages ::WIMLIB_PROGRESS_MSG_SCAN_BEGIN,
	 * ::WIMLIB_PROGRESS_MSG_SCAN_DENTRY, and
	 * ::WIMLIB_PROGRESS_MSG_SCAN_END.  */
	struct wimlib_progress_info_scan {

		/** Top-level directory being scanned; or, when capturing an NTFS
		 * volume with ::WIMLIB_ADD_FLAG_NTFS, this is instead the path
		 * to the file or block device that contains the NTFS volume
		 * being scanned.  */
		const wimlib_tchar *source;

		/** Path to the file (or directory) that has been scanned, valid
		 * on ::WIMLIB_PROGRESS_MSG_SCAN_DENTRY.  When capturing an NTFS
		 * volume with ::WIMLIB_ADD_FLAG_NTFS, this path will be
		 * relative to the root of the NTFS volume.  */
		const wimlib_tchar *cur_path;

		/** Dentry scan status, valid on
		 * ::WIMLIB_PROGRESS_MSG_SCAN_DENTRY.  */
		enum {
			/** File looks okay and will be captured.  */
			WIMLIB_SCAN_DENTRY_OK = 0,

			/** File is being excluded from capture due to the
			 * capture configuration.  */
			WIMLIB_SCAN_DENTRY_EXCLUDED = 1,

			/** File is being excluded from capture due to being of
			 * an unsupported type.  */
			WIMLIB_SCAN_DENTRY_UNSUPPORTED = 2,

			/** The file is an absolute symbolic link or junction
			 * that points into the capture directory, and
			 * reparse-point fixups are enabled, so its target is
			 * being adjusted.  (Reparse point fixups can be
			 * disabled with the flag ::WIMLIB_ADD_FLAG_NORPFIX.)
			 */
			WIMLIB_SCAN_DENTRY_FIXED_SYMLINK = 3,

			/** Reparse-point fixups are enabled, but the file is an
			 * absolute symbolic link or junction that does
			 * <b>not</b> point into the capture directory, so its
			 * target is <b>not</b> being adjusted.  */
			WIMLIB_SCAN_DENTRY_NOT_FIXED_SYMLINK = 4,
		} status;

		union {
			/** Target path in the image.  Only valid on messages
			 * ::WIMLIB_PROGRESS_MSG_SCAN_BEGIN and
			 * ::WIMLIB_PROGRESS_MSG_SCAN_END.  */
			const wimlib_tchar *wim_target_path;

			/** For ::WIMLIB_PROGRESS_MSG_SCAN_DENTRY and a status
			 * of @p WIMLIB_SCAN_DENTRY_FIXED_SYMLINK or @p
			 * WIMLIB_SCAN_DENTRY_NOT_FIXED_SYMLINK, this is the
			 * target of the absolute symbolic link or junction.  */
			const wimlib_tchar *symlink_target;
		};

		/** The number of directories scanned so far, not counting
		 * excluded/unsupported files.  */
		uint64_t num_dirs_scanned;

		/** The number of non-directories scanned so far, not counting
		 * excluded/unsupported files.  */
		uint64_t num_nondirs_scanned;

		/** The number of bytes of file data detected so far, not
		 * counting excluded/unsupported files.  */
		uint64_t num_bytes_scanned;
	} scan;

	/** Valid on messages
	 * ::WIMLIB_PROGRESS_MSG_EXTRACT_SPWM_PART_BEGIN,
	 * ::WIMLIB_PROGRESS_MSG_EXTRACT_IMAGE_BEGIN,
	 * ::WIMLIB_PROGRESS_MSG_EXTRACT_TREE_BEGIN,
	 * ::WIMLIB_PROGRESS_MSG_EXTRACT_FILE_STRUCTURE,
	 * ::WIMLIB_PROGRESS_MSG_EXTRACT_STREAMS,
	 * ::WIMLIB_PROGRESS_MSG_EXTRACT_METADATA,
	 * ::WIMLIB_PROGRESS_MSG_EXTRACT_TREE_END, and
	 * ::WIMLIB_PROGRESS_MSG_EXTRACT_IMAGE_END.
	 *
	 * Note: most of the time of an extraction operation will be spent
	 * extracting file data, and the application will receive
	 * ::WIMLIB_PROGRESS_MSG_EXTRACT_STREAMS during this time.  Using @p
	 * completed_bytes and @p total_bytes, the application can calculate a
	 * percentage complete.  However, there is no way for applications to
	 * know which file is currently being extracted.  This is by design
	 * because the best way to complete the extraction operation is not
	 * necessarily file-by-file.
	 */
	struct wimlib_progress_info_extract {

		/** The 1-based index of the image from which files are being
		 * extracted.  */
		uint32_t image;

		/** Extraction flags being used.  */
		uint32_t extract_flags;

		/** If the ::WIMStruct from which the extraction being performed
		 * has a backing file, then this is an absolute path to that
		 * backing file.  Otherwise, this is @c NULL.  */
		const wimlib_tchar *wimfile_name;

		/** Name of the image from which files are being extracted, or
		 * the empty string if the image is unnamed.  */
		const wimlib_tchar *image_name;

		/** Path to the directory or NTFS volume to which the files are
		 * being extracted.  */
		const wimlib_tchar *target;

		/** Reserved.  */
		const wimlib_tchar *reserved;

		/** The number of bytes of file data that will be extracted.  */
		uint64_t total_bytes;

		/** The number of bytes of file data that have been extracted so
		 * far.  This starts at 0 and ends at @p total_bytes.  */
		uint64_t completed_bytes;

		/** The number of file streams that will be extracted.  This
		 * will often be similar to the "number of files", but for
		 * several reasons (hard links, named data streams, empty files,
		 * etc.) it can be different.  */
		uint64_t total_streams;

		/** The number of file streams that have been extracted so far.
		 * This starts at 0 and ends at @p total_streams.  */
		uint64_t completed_streams;

		/** Currently only used for
		 * ::WIMLIB_PROGRESS_MSG_EXTRACT_SPWM_PART_BEGIN.  */
		uint32_t part_number;

		/** Currently only used for
		 * ::WIMLIB_PROGRESS_MSG_EXTRACT_SPWM_PART_BEGIN.  */
		uint32_t total_parts;

		/** Currently only used for
		 * ::WIMLIB_PROGRESS_MSG_EXTRACT_SPWM_PART_BEGIN.  */
		uint8_t guid[WIMLIB_GUID_LEN];

		/** For ::WIMLIB_PROGRESS_MSG_EXTRACT_FILE_STRUCTURE and
		 * ::WIMLIB_PROGRESS_MSG_EXTRACT_METADATA messages, this is the
		 * number of files that have been processed so far.  Once the
		 * corresponding phase of extraction is complete, this value
		 * will be equal to @c end_file_count.  */
		uint64_t current_file_count;

		/** For ::WIMLIB_PROGRESS_MSG_EXTRACT_FILE_STRUCTURE and
		 * ::WIMLIB_PROGRESS_MSG_EXTRACT_METADATA messages, this is
		 * total number of files that will be processed.
		 *
		 * This number is provided for informational purposes only, e.g.
		 * for a progress bar.  This number will not necessarily be
		 * equal to the number of files actually being extracted.  This
		 * is because extraction backends are free to implement an
		 * extraction algorithm that might be more efficient than
		 * processing every file in the "extract file structure" and
		 * "extract file metadata" phases.  For example, the current
		 * implementation of the UNIX extraction backend will create
		 * files on-demand during the "extract file data" phase.
		 * Therefore, when using that particular extraction backend, @p
		 * end_file_count will only include directories and empty files.
		 */
		uint64_t end_file_count;
	} extract;

	/** Valid on messages ::WIMLIB_PROGRESS_MSG_RENAME. */
	struct wimlib_progress_info_rename {
		/** Name of the temporary file that the WIM was written to. */
		const wimlib_tchar *from;

		/** Name of the original WIM file to which the temporary file is
		 * being renamed. */
		const wimlib_tchar *to;
	} rename;

	/** Valid on messages ::WIMLIB_PROGRESS_MSG_UPDATE_BEGIN_COMMAND and
	 * ::WIMLIB_PROGRESS_MSG_UPDATE_END_COMMAND. */
	struct wimlib_progress_info_update {
		/** Pointer to the update command that will be executed or has
		 * just been executed. */
		const struct wimlib_update_command *command;

		/** Number of update commands that have been completed so far.
		 */
		size_t completed_commands;

		/** Number of update commands that are being executed as part of
		 * this call to wimlib_update_image(). */
		size_t total_commands;
	} update;

	/** Valid on messages ::WIMLIB_PROGRESS_MSG_VERIFY_INTEGRITY and
	 * ::WIMLIB_PROGRESS_MSG_CALC_INTEGRITY. */
	struct wimlib_progress_info_integrity {

		/** The number of bytes in the WIM file that are covered by
		 * integrity checks.  */
		uint64_t total_bytes;

		/** The number of bytes that have been checksummed so far.  This
		 * starts at 0 and ends at @p total_bytes.  */
		uint64_t completed_bytes;

		/** The number of individually checksummed "chunks" the
		 * integrity-checked region is divided into.  */
		uint32_t total_chunks;

		/** The number of chunks that have been checksummed so far.
		 * This starts at 0 and ends at @p total_chunks.  */
		uint32_t completed_chunks;

		/** The size of each individually checksummed "chunk" in the
		 * integrity-checked region.  */
		uint32_t chunk_size;

		/** For ::WIMLIB_PROGRESS_MSG_VERIFY_INTEGRITY messages, this is
		 * the path to the WIM file being checked.  */
		const wimlib_tchar *filename;
	} integrity;

	/** Valid on messages ::WIMLIB_PROGRESS_MSG_SPLIT_BEGIN_PART and
	 * ::WIMLIB_PROGRESS_MSG_SPLIT_END_PART. */
	struct wimlib_progress_info_split {
		/** Total size of the original WIM's file and metadata resources
		 * (compressed). */
		uint64_t total_bytes;

		/** Number of bytes of file and metadata resources that have
		 * been copied out of the original WIM so far.  Will be 0
		 * initially, and equal to @p total_bytes at the end. */
		uint64_t completed_bytes;

		/** Number of the split WIM part that is about to be started
		 * (::WIMLIB_PROGRESS_MSG_SPLIT_BEGIN_PART) or has just been
		 * finished (::WIMLIB_PROGRESS_MSG_SPLIT_END_PART). */
		unsigned cur_part_number;

		/** Total number of split WIM parts that are being written.  */
		unsigned total_parts;

		/** Name of the split WIM part that is about to be started
		 * (::WIMLIB_PROGRESS_MSG_SPLIT_BEGIN_PART) or has just been
		 * finished (::WIMLIB_PROGRESS_MSG_SPLIT_END_PART).  Since
		 * wimlib v1.7.0, the library user may change this when
		 * receiving ::WIMLIB_PROGRESS_MSG_SPLIT_BEGIN_PART in order to
		 * cause the next split WIM part to be written to a different
		 * location.  */
		wimlib_tchar *part_name;
	} split;

	/** Valid on messages ::WIMLIB_PROGRESS_MSG_REPLACE_FILE_IN_WIM  */
	struct wimlib_progress_info_replace {
		/** Path to the file in the image that is being replaced  */
		const wimlib_tchar *path_in_wim;
	} replace;

	/** Valid on messages ::WIMLIB_PROGRESS_MSG_WIMBOOT_EXCLUDE  */
	struct wimlib_progress_info_wimboot_exclude {
		/** Path to the file in the image  */
		const wimlib_tchar *path_in_wim;

		/** Path to which the file is being extracted  */
		const wimlib_tchar *extraction_path;
	} wimboot_exclude;

	/** Valid on messages ::WIMLIB_PROGRESS_MSG_UNMOUNT_BEGIN.  */
	struct wimlib_progress_info_unmount {
		/** Path to directory being unmounted  */
		const wimlib_tchar *mountpoint;

		/** Path to WIM file being unmounted  */
		const wimlib_tchar *mounted_wim;

		/** 1-based index of image being unmounted.  */
		uint32_t mounted_image;

		/** Flags that were passed to wimlib_mount_image() when the
		 * mountpoint was set up.  */
		uint32_t mount_flags;

		/** Flags passed to wimlib_unmount_image().  */
		uint32_t unmount_flags;
	} unmount;

	/** Valid on messages ::WIMLIB_PROGRESS_MSG_DONE_WITH_FILE.  */
	struct wimlib_progress_info_done_with_file {
		/**
		 * Path to the file whose data has been written to the WIM file,
		 * or is currently being asynchronously compressed in memory,
		 * and therefore is no longer needed by wimlib.
		 *
		 * WARNING: The file data will not actually be accessible in the
		 * WIM file until the WIM file has been completely written.
		 * Ordinarily you should <b>not</b> treat this message as a
		 * green light to go ahead and delete the specified file, since
		 * that would result in data loss if the WIM file cannot be
		 * successfully created for any reason.
		 *
		 * If a file has multiple names (hard links),
		 * ::WIMLIB_PROGRESS_MSG_DONE_WITH_FILE will only be received
		 * for one name.  Also, this message will not be received for
		 * empty files or reparse points (or symbolic links), unless
		 * they have nonempty named data streams.
		 */
		const wimlib_tchar *path_to_file;
	} done_with_file;

	/** Valid on messages ::WIMLIB_PROGRESS_MSG_BEGIN_VERIFY_IMAGE and
	 * ::WIMLIB_PROGRESS_MSG_END_VERIFY_IMAGE.  */
	struct wimlib_progress_info_verify_image {
		const wimlib_tchar *wimfile;
		uint32_t total_images;
		uint32_t current_image;
	} verify_image;

	/** Valid on messages ::WIMLIB_PROGRESS_MSG_VERIFY_STREAMS.  */
	struct wimlib_progress_info_verify_streams {
		const wimlib_tchar *wimfile;
		uint64_t total_streams;
		uint64_t total_bytes;
		uint64_t completed_streams;
		uint64_t completed_bytes;
	} verify_streams;

	/** Valid on messages ::WIMLIB_PROGRESS_MSG_TEST_FILE_EXCLUSION.  */
	struct wimlib_progress_info_test_file_exclusion {

		/**
		 * Path to the file for which exclusion is being tested.
		 *
		 * UNIX capture mode:  The path will be a standard relative or
		 * absolute UNIX filesystem path.
		 *
		 * NTFS-3G capture mode:  The path will be given relative to the
		 * root of the NTFS volume, with a leading slash.
		 *
		 * Windows capture mode:  The path will be a Win32 namespace
		 * path to the file.
		 */
		const wimlib_tchar *path;

		/**
		 * Indicates whether the file or directory will be excluded from
		 * capture or not.  This will be <c>false</c> by default.  The
		 * progress function can set this to <c>true</c> if it decides
		 * that the file needs to be excluded.
		 */
		bool will_exclude;
	} test_file_exclusion;

	/** Valid on messages ::WIMLIB_PROGRESS_MSG_HANDLE_ERROR.  */
	struct wimlib_progress_info_handle_error {

		/** Path to the file for which the error occurred, or NULL if
		 * not relevant.  */
		const wimlib_tchar *path;

		/** The wimlib error code associated with the error.  */
		int error_code;

		/**
		 * Indicates whether the error will be ignored or not.  This
		 * will be <c>false</c> by default; the progress function may
		 * set it to <c>true</c>.
		 */
		bool will_ignore;
	} handle_error;
};

/**
 * A user-supplied function that will be called periodically during certain WIM
 * operations.
 *
 * The first argument will be the type of operation that is being performed or
 * is about to be started or has been completed.
 *
 * The second argument will be a pointer to one of a number of structures
 * depending on the first argument.  It may be @c NULL for some message types.
 * Note that although this argument is not @c const, users should not modify it
 * except in explicitly documented cases.
 *
 * The third argument will be a user-supplied value that was provided when
 * registering or specifying the progress function.
 *
 * This function must return one of the ::wimlib_progress_status values.  By
 * default, you should return ::WIMLIB_PROGRESS_STATUS_CONTINUE (0).
 */
typedef enum wimlib_progress_status
	(*wimlib_progress_func_t)(enum wimlib_progress_msg msg_type,
				  union wimlib_progress_info *info,
				  void *progctx);

/** @} */
/** @addtogroup G_modifying_wims
 * @{ */

/** An array of these structures is passed to wimlib_add_image_multisource() to
 * specify the sources from which to create a WIM image. */
struct wimlib_capture_source {
	/** Absolute or relative path to a file or directory on the external
	 * filesystem to be included in the image. */
	wimlib_tchar *fs_source_path;

	/** Destination path in the image.  To specify the root directory of the
	 * image, use ::WIMLIB_WIM_ROOT_PATH.  */
	wimlib_tchar *wim_target_path;

	/** Reserved; set to 0. */
	long reserved;
};

/** Set or unset the "readonly" WIM header flag (<c>WIM_HDR_FLAG_READONLY</c> in
 * Microsoft's documentation), based on the ::wimlib_wim_info.is_marked_readonly
 * member of the @p info parameter.  This is distinct from basic file
 * permissions; this flag can be set on a WIM file that is physically writable.
 *
 * wimlib disallows modifying on-disk WIM files with the readonly flag set.
 * However, wimlib_overwrite() with ::WIMLIB_WRITE_FLAG_IGNORE_READONLY_FLAG
 * will override this --- and in fact, this is necessary to set the readonly
 * flag persistently on an existing WIM file.
 */
#define WIMLIB_CHANGE_READONLY_FLAG		0x00000001

/** Set the GUID (globally unique identifier) of the WIM file to the value
 * specified in ::wimlib_wim_info.guid of the @p info parameter. */
#define WIMLIB_CHANGE_GUID			0x00000002

/** Change the bootable image of the WIM to the value specified in
 * ::wimlib_wim_info.boot_index of the @p info parameter.  */
#define WIMLIB_CHANGE_BOOT_INDEX		0x00000004

/** Change the <c>WIM_HDR_FLAG_RP_FIX</c> flag of the WIM file to the value
 * specified in ::wimlib_wim_info.has_rpfix of the @p info parameter.  This flag
 * generally indicates whether an image in the WIM has been captured with
 * reparse-point fixups enabled.  wimlib also treats this flag as specifying
 * whether to do reparse-point fixups by default when capturing or applying WIM
 * images.  */
#define WIMLIB_CHANGE_RPFIX_FLAG		0x00000008

/** @} */

/** @addtogroup G_wim_information  */

/** @{ */

/**
 * General information about a WIM file.
 *
 * This info can also be requested for a ::WIMStruct that does not have a
 * backing file.  In this case, fields that only make sense given a backing file
 * are set to default values.
 */
struct wimlib_wim_info {

	/** The globally unique identifier for this WIM.  (Note: all parts of a
	 * split WIM normally have identical GUIDs.)  */
	uint8_t guid[WIMLIB_GUID_LEN];

	/** The number of images in this WIM file.  */
	uint32_t image_count;

	/** The 1-based index of the bootable image in this WIM file, or 0 if no
	 * image is bootable.  */
	uint32_t boot_index;

	/** The version of the WIM file format used in this WIM file.  */
	uint32_t wim_version;

	/** The default compression chunk size of resources in this WIM file.
	 */
	uint32_t chunk_size;

	/** For split WIMs, the 1-based index of this part within the split WIM;
	 * otherwise 1.  */
	uint16_t part_number;

	/** For split WIMs, the total number of parts in the split WIM;
	 * otherwise 1.  */
	uint16_t total_parts;

	/** The default compression type of resources in this WIM file, as one
	 * of the ::wimlib_compression_type constants.  */
	int32_t compression_type;

	/** The size of this WIM file in bytes, excluding the XML data and
	 * integrity table.  */
	uint64_t total_bytes;

	/** 1 iff this WIM file has an integrity table.  */
	uint32_t has_integrity_table : 1;

	/** 1 iff this info struct is for a ::WIMStruct that has a backing file.
	 */
	uint32_t opened_from_file : 1;

	/** 1 iff this WIM file is considered readonly for any reason (e.g. the
	 * "readonly" header flag is set, or this is part of a split WIM, or
	 * filesystem permissions deny writing)  */
	uint32_t is_readonly : 1;

	/** 1 iff the "reparse point fix" flag is set in this WIM's header  */
	uint32_t has_rpfix : 1;

	/** 1 iff the "readonly" flag is set in this WIM's header  */
	uint32_t is_marked_readonly : 1;

	/** 1 iff the "spanned" flag is set in this WIM's header  */
	uint32_t spanned : 1;

	/** 1 iff the "write in progress" flag is set in this WIM's header  */
	uint32_t write_in_progress : 1;

	/** 1 iff the "metadata only" flag is set in this WIM's header  */
	uint32_t metadata_only : 1;

	/** 1 iff the "resource only" flag is set in this WIM's header  */
	uint32_t resource_only : 1;

	/** 1 iff this WIM file is pipable (see ::WIMLIB_WRITE_FLAG_PIPABLE).  */
	uint32_t pipable : 1;
	uint32_t reserved_flags : 22;
	uint32_t reserved[9];
};

/**
 * Information about a "blob", which is a fixed length sequence of binary data.
 * Each nonempty stream of each file in a WIM image is associated with a blob.
 * Blobs are deduplicated within a WIM file.
 *
 * TODO: this struct needs to be renamed, and perhaps made into a union since
 * there are several cases.  I'll try to list them below:
 *
 * 1. The blob is "missing", meaning that it is referenced by hash but not
 *    actually present in the WIM file.  In this case we only know the
 *    sha1_hash.  This case can only occur with wimlib_iterate_dir_tree(), never
 *    wimlib_iterate_lookup_table().
 *
 * 2. Otherwise we know the uncompressed_size, the reference_count, and the
 *    is_metadata flag.  In addition:
 *
 *    A. If the blob is located in a non-solid WIM resource, then we also know
 *       the sha1_hash, compressed_size, and offset.
 *
 *    B. If the blob is located in a solid WIM resource, then we also know the
 *       sha1_hash, offset, raw_resource_offset_in_wim,
 *       raw_resource_compressed_size, and raw_resource_uncompressed_size.  But
 *       the "offset" is actually the offset in the uncompressed solid resource
 *       rather than the offset from the beginning of the WIM file.
 *
 *    C. If the blob is *not* located in any type of WIM resource, for example
 *       if it's in a external file that was scanned by wimlib_add_image(), then
 *       we usually won't know any more information.  The sha1_hash might be
 *       known, and prior to wimlib v1.13.6 it always was; however, in wimlib
 *       v1.13.6 and later, the sha1_hash might not be known in this case.
 *
 * Unknown or irrelevant fields are left zeroed.
 */
struct wimlib_resource_entry {

	/** If this blob is not missing, then this is the uncompressed size of
	 * this blob in bytes.  */
	uint64_t uncompressed_size;

	/** If this blob is located in a non-solid WIM resource, then this is
	 * the compressed size of that resource.  */
	uint64_t compressed_size;

	/** If this blob is located in a non-solid WIM resource, then this is
	 * the offset of that resource within the WIM file containing it.  If
	 * this blob is located in a solid WIM resource, then this is the offset
	 * of this blob within that solid resource when uncompressed.  */
	uint64_t offset;

	/** If this blob is located in a WIM resource, then this is the SHA-1
	 * message digest of the blob's uncompressed contents.  */
	uint8_t sha1_hash[20];

	/** If this blob is located in a WIM resource, then this is the part
	 * number of the WIM file containing it.  */
	uint32_t part_number;

	/** If this blob is not missing, then this is the number of times this
	 * blob is referenced over all images in the WIM.  This number is not
	 * guaranteed to be correct.  */
	uint32_t reference_count;

	/** 1 iff this blob is located in a non-solid compressed WIM resource.
	 */
	uint32_t is_compressed : 1;

	/** 1 iff this blob contains the metadata for an image.  */
	uint32_t is_metadata : 1;

	uint32_t is_free : 1;
	uint32_t is_spanned : 1;

	/** 1 iff a blob with this hash was not found in the blob lookup table
	 * of the ::WIMStruct.  This normally implies a missing call to
	 * wimlib_reference_resource_files() or wimlib_reference_resources(). */
	uint32_t is_missing : 1;

	/** 1 iff this blob is located in a solid resource.  */
	uint32_t packed : 1;

	uint32_t reserved_flags : 26;

	/** If this blob is located in a solid WIM resource, then this is the
	 * offset of that solid resource within the WIM file containing it.  */
	uint64_t raw_resource_offset_in_wim;

	/** If this blob is located in a solid WIM resource, then this is the
	 * compressed size of that solid resource.  */
	uint64_t raw_resource_compressed_size;

	/** If this blob is located in a solid WIM resource, then this is the
	 * uncompressed size of that solid resource.  */
	uint64_t raw_resource_uncompressed_size;

	uint64_t reserved[1];
};

/**
 * Information about a stream of a particular file in the WIM.
 *
 * Normally, only WIM images captured from NTFS filesystems will have multiple
 * streams per file.  In practice, this is a rarely used feature of the
 * filesystem.
 *
 * TODO: the library now explicitly tracks stream types, which allows it to have
 * multiple unnamed streams (e.g. both a reparse point stream and unnamed data
 * stream).  However, this isn't yet exposed by wimlib_iterate_dir_tree().
 */
struct wimlib_stream_entry {

	/** Name of the stream, or NULL if the stream is unnamed.  */
	const wimlib_tchar *stream_name;

	/** Info about this stream's data, such as its hash and size if known.*/
	struct wimlib_resource_entry resource;

	uint64_t reserved[4];
};

/**
 * Since wimlib v1.9.1: an object ID, which is an extra piece of metadata that
 * may be associated with a file on NTFS filesystems.  See:
 * https://msdn.microsoft.com/en-us/library/windows/desktop/aa363997(v=vs.85).aspx
 */
struct wimlib_object_id {
	uint8_t object_id[WIMLIB_GUID_LEN];
	uint8_t birth_volume_id[WIMLIB_GUID_LEN];
	uint8_t birth_object_id[WIMLIB_GUID_LEN];
	uint8_t domain_id[WIMLIB_GUID_LEN];
};

/** Structure passed to the wimlib_iterate_dir_tree() callback function.
 * Roughly, the information about a "file" in the WIM image --- but really a
 * directory entry ("dentry") because hard links are allowed.  The
 * hard_link_group_id field can be used to distinguish actual file inodes.  */
struct wimlib_dir_entry {
	/** Name of the file, or NULL if this file is unnamed.  Only the root
	 * directory of an image will be unnamed.  */
	const wimlib_tchar *filename;

	/** 8.3 name (or "DOS name", or "short name") of this file; or NULL if
	 * this file has no such name.  */
	const wimlib_tchar *dos_name;

	/** Full path to this file within the image.  Path separators will be
	 * ::WIMLIB_WIM_PATH_SEPARATOR.  */
	const wimlib_tchar *full_path;

	/** Depth of this directory entry, where 0 is the root, 1 is the root's
	 * children, ..., etc. */
	size_t depth;

	/** Pointer to the security descriptor for this file, in Windows
	 * SECURITY_DESCRIPTOR_RELATIVE format, or NULL if this file has no
	 * security descriptor.  */
	const char *security_descriptor;

	/** Size of the above security descriptor, in bytes.  */
	size_t security_descriptor_size;

#define WIMLIB_FILE_ATTRIBUTE_READONLY            0x00000001
#define WIMLIB_FILE_ATTRIBUTE_HIDDEN              0x00000002
#define WIMLIB_FILE_ATTRIBUTE_SYSTEM              0x00000004
#define WIMLIB_FILE_ATTRIBUTE_DIRECTORY           0x00000010
#define WIMLIB_FILE_ATTRIBUTE_ARCHIVE             0x00000020
#define WIMLIB_FILE_ATTRIBUTE_DEVICE              0x00000040
#define WIMLIB_FILE_ATTRIBUTE_NORMAL              0x00000080
#define WIMLIB_FILE_ATTRIBUTE_TEMPORARY           0x00000100
#define WIMLIB_FILE_ATTRIBUTE_SPARSE_FILE         0x00000200
#define WIMLIB_FILE_ATTRIBUTE_REPARSE_POINT       0x00000400
#define WIMLIB_FILE_ATTRIBUTE_COMPRESSED          0x00000800
#define WIMLIB_FILE_ATTRIBUTE_OFFLINE             0x00001000
#define WIMLIB_FILE_ATTRIBUTE_NOT_CONTENT_INDEXED 0x00002000
#define WIMLIB_FILE_ATTRIBUTE_ENCRYPTED           0x00004000
#define WIMLIB_FILE_ATTRIBUTE_VIRTUAL             0x00010000
	/** File attributes, such as whether the file is a directory or not.
	 * These are the "standard" Windows FILE_ATTRIBUTE_* values, although in
	 * wimlib.h they are defined as WIMLIB_FILE_ATTRIBUTE_* for convenience
	 * on other platforms.  */
	uint32_t attributes;

#define WIMLIB_REPARSE_TAG_RESERVED_ZERO	0x00000000
#define WIMLIB_REPARSE_TAG_RESERVED_ONE		0x00000001
#define WIMLIB_REPARSE_TAG_MOUNT_POINT		0xA0000003
#define WIMLIB_REPARSE_TAG_HSM			0xC0000004
#define WIMLIB_REPARSE_TAG_HSM2			0x80000006
#define WIMLIB_REPARSE_TAG_DRIVER_EXTENDER	0x80000005
#define WIMLIB_REPARSE_TAG_SIS			0x80000007
#define WIMLIB_REPARSE_TAG_DFS			0x8000000A
#define WIMLIB_REPARSE_TAG_DFSR			0x80000012
#define WIMLIB_REPARSE_TAG_FILTER_MANAGER	0x8000000B
#define WIMLIB_REPARSE_TAG_WOF			0x80000017
#define WIMLIB_REPARSE_TAG_SYMLINK		0xA000000C
	/** If the file is a reparse point (FILE_ATTRIBUTE_REPARSE_POINT set in
	 * the attributes), this will give the reparse tag.  This tells you
	 * whether the reparse point is a symbolic link, junction point, or some
	 * other, more unusual kind of reparse point.  */
	uint32_t reparse_tag;

	/** Number of links to this file's inode (hard links).
	 *
	 * Currently, this will always be 1 for directories.  However, it can be
	 * greater than 1 for nondirectory files.  */
	uint32_t num_links;

	/** Number of named data streams this file has.  Normally 0.  */
	uint32_t num_named_streams;

	/** A unique identifier for this file's inode.  However, as a special
	 * case, if the inode only has a single link (@p num_links == 1), this
	 * value may be 0.
	 *
	 * Note: if a WIM image is captured from a filesystem, this value is not
	 * guaranteed to be the same as the original number of the inode on the
	 * filesystem.  */
	uint64_t hard_link_group_id;

	/** Time this file was created.  */
	struct wimlib_timespec creation_time;

	/** Time this file was last written to.  */
	struct wimlib_timespec last_write_time;

	/** Time this file was last accessed.  */
	struct wimlib_timespec last_access_time;

	/** The UNIX user ID of this file.  This is a wimlib extension.
	 *
	 * This field is only valid if @p unix_mode != 0.  */
	uint32_t unix_uid;

	/** The UNIX group ID of this file.  This is a wimlib extension.
	 *
	 * This field is only valid if @p unix_mode != 0.  */
	uint32_t unix_gid;

	/** The UNIX mode of this file.  This is a wimlib extension.
	 *
	 * If this field is 0, then @p unix_uid, @p unix_gid, @p unix_mode, and
	 * @p unix_rdev are all unknown (fields are not present in the WIM
	 * image).  */
	uint32_t unix_mode;

	/** The UNIX device ID (major and minor number) of this file.  This is a
	 * wimlib extension.
	 *
	 * This field is only valid if @p unix_mode != 0.  */
	uint32_t unix_rdev;

	/* The object ID of this file, if any.  Only valid if
	 * object_id.object_id is not all zeroes.  */
	struct wimlib_object_id object_id;

	/** High 32 bits of the seconds portion of the creation timestamp,
	 * filled in if @p wimlib_timespec.tv_sec is only 32-bit. */
	int32_t creation_time_high;

	/** High 32 bits of the seconds portion of the last write timestamp,
	 * filled in if @p wimlib_timespec.tv_sec is only 32-bit. */
	int32_t last_write_time_high;

	/** High 32 bits of the seconds portion of the last access timestamp,
	 * filled in if @p wimlib_timespec.tv_sec is only 32-bit. */
	int32_t last_access_time_high;

	int32_t reserved2;

	uint64_t reserved[4];

	/**
	 * Variable-length array of streams that make up this file.
	 *
	 * The first entry will always exist and will correspond to the unnamed
	 * data stream (default file contents), so it will have <c>stream_name
	 * == NULL</c>.  Alternatively, for reparse point files, the first entry
	 * will correspond to the reparse data stream.  Alternatively, for
	 * encrypted files, the first entry will correspond to the encrypted
	 * data.
	 *
	 * Then, following the first entry, there be @p num_named_streams
	 * additional entries that specify the named data streams, if any, each
	 * of which will have <c>stream_name != NULL</c>.
	 */
	struct wimlib_stream_entry streams[];
};

/**
 * Type of a callback function to wimlib_iterate_dir_tree().  Must return 0 on
 * success.
 */
typedef int (*wimlib_iterate_dir_tree_callback_t)(const struct wimlib_dir_entry *dentry,
						  void *user_ctx);

/**
 * Type of a callback function to wimlib_iterate_lookup_table().  Must return 0
 * on success.
 */
typedef int (*wimlib_iterate_lookup_table_callback_t)(const struct wimlib_resource_entry *resource,
						      void *user_ctx);

/** For wimlib_iterate_dir_tree(): Iterate recursively on children rather than
 * just on the specified path. */
#define WIMLIB_ITERATE_DIR_TREE_FLAG_RECURSIVE 0x00000001

/** For wimlib_iterate_dir_tree(): Don't iterate on the file or directory
 * itself; only its children (in the case of a non-empty directory) */
#define WIMLIB_ITERATE_DIR_TREE_FLAG_CHILDREN  0x00000002

/** Return ::WIMLIB_ERR_RESOURCE_NOT_FOUND if any file data blobs needed to fill
 * in the ::wimlib_resource_entry's for the iteration cannot be found in the
 * blob lookup table of the ::WIMStruct.  The default behavior without this flag
 * is to fill in the @ref wimlib_resource_entry::sha1_hash "sha1_hash" and set
 * the @ref wimlib_resource_entry::is_missing "is_missing" flag.  */
#define WIMLIB_ITERATE_DIR_TREE_FLAG_RESOURCES_NEEDED  0x00000004


/** @} */
/** @addtogroup G_modifying_wims
 * @{ */

/** UNIX-like systems only: Directly capture an NTFS volume rather than a
 * generic directory.  This requires that wimlib was compiled with support for
 * libntfs-3g.
 *
 * This flag cannot be combined with ::WIMLIB_ADD_FLAG_DEREFERENCE or
 * ::WIMLIB_ADD_FLAG_UNIX_DATA.
 *
 * Do not use this flag on Windows, where wimlib already supports all
 * Windows-native filesystems, including NTFS, through the Windows APIs.  */
#define WIMLIB_ADD_FLAG_NTFS			0x00000001

/** Follow symbolic links when scanning the directory tree.  Currently only
 * supported on UNIX-like systems.  */
#define WIMLIB_ADD_FLAG_DEREFERENCE		0x00000002

/** Call the progress function with the message
 * ::WIMLIB_PROGRESS_MSG_SCAN_DENTRY when each directory or file has been
 * scanned.  */
#define WIMLIB_ADD_FLAG_VERBOSE			0x00000004

/** Mark the image being added as the bootable image of the WIM.  This flag is
 * valid only for wimlib_add_image() and wimlib_add_image_multisource().
 *
 * Note that you can also change the bootable image of a WIM using
 * wimlib_set_wim_info().
 *
 * Note: ::WIMLIB_ADD_FLAG_BOOT does something different from, and independent
 * from, ::WIMLIB_ADD_FLAG_WIMBOOT.  */
#define WIMLIB_ADD_FLAG_BOOT			0x00000008

/** UNIX-like systems only: Store the UNIX owner, group, mode, and device ID
 * (major and minor number) of each file.  In addition, capture special files
 * such as device nodes and FIFOs.  Since wimlib v1.11.0, on Linux also capture
 * extended attributes.  See the documentation for the <b>--unix-data</b> option
 * to <b>wimcapture</b> for more information.  */
#define WIMLIB_ADD_FLAG_UNIX_DATA		0x00000010

/** Do not capture security descriptors.  Only has an effect in NTFS-3G capture
 * mode, or in Windows native builds.  */
#define WIMLIB_ADD_FLAG_NO_ACLS			0x00000020

/** Fail immediately if the full security descriptor of any file or directory
 * cannot be accessed.  Only has an effect in Windows native builds.  The
 * default behavior without this flag is to first try omitting the SACL from the
 * security descriptor, then to try omitting the security descriptor entirely.
 */
#define WIMLIB_ADD_FLAG_STRICT_ACLS		0x00000040

/** Call the progress function with the message
 * ::WIMLIB_PROGRESS_MSG_SCAN_DENTRY when a directory or file is excluded from
 * capture.  This is a subset of the messages provided by
 * ::WIMLIB_ADD_FLAG_VERBOSE.  */
#define WIMLIB_ADD_FLAG_EXCLUDE_VERBOSE		0x00000080

/** Reparse-point fixups:  Modify absolute symbolic links (and junctions, in the
 * case of Windows) that point inside the directory being captured to instead be
 * absolute relative to the directory being captured.
 *
 * Without this flag, the default is to do reparse-point fixups if
 * <c>WIM_HDR_FLAG_RP_FIX</c> is set in the WIM header or if this is the first
 * image being added.  */
#define WIMLIB_ADD_FLAG_RPFIX			0x00000100

/** Don't do reparse point fixups.  See ::WIMLIB_ADD_FLAG_RPFIX.  */
#define WIMLIB_ADD_FLAG_NORPFIX			0x00000200

/** Do not automatically exclude unsupported files or directories from capture,
 * such as encrypted files in NTFS-3G capture mode, or device files and FIFOs on
 * UNIX-like systems when not also using ::WIMLIB_ADD_FLAG_UNIX_DATA.  Instead,
 * fail with ::WIMLIB_ERR_UNSUPPORTED_FILE when such a file is encountered.  */
#define WIMLIB_ADD_FLAG_NO_UNSUPPORTED_EXCLUDE	0x00000400

/**
 * Automatically select a capture configuration appropriate for capturing
 * filesystems containing Windows operating systems.  For example,
 * <c>/pagefile.sys</c> and <c>"/System Volume Information"</c> will be
 * excluded.
 *
 * When this flag is specified, the corresponding @p config parameter (for
 * wimlib_add_image()) or member (for wimlib_update_image()) must be @c NULL.
 * Otherwise, ::WIMLIB_ERR_INVALID_PARAM will be returned.
 *
 * Note that the default behavior--- that is, when neither
 * ::WIMLIB_ADD_FLAG_WINCONFIG nor ::WIMLIB_ADD_FLAG_WIMBOOT is specified and @p
 * config is @c NULL--- is to use no capture configuration, meaning that no
 * files are excluded from capture.
 */
#define WIMLIB_ADD_FLAG_WINCONFIG		0x00000800

/**
 * Capture image as "WIMBoot compatible".  In addition, if no capture
 * configuration file is explicitly specified use the capture configuration file
 * <c>$SOURCE/Windows/System32/WimBootCompress.ini</c> if it exists, where
 * <c>$SOURCE</c> is the directory being captured; or, if a capture
 * configuration file is explicitly specified, use it and also place it at
 * <c>/Windows/System32/WimBootCompress.ini</c> in the WIM image.
 *
 * This flag does not, by itself, change the compression type or chunk size.
 * Before writing the WIM file, you may wish to set the compression format to
 * be the same as that used by WIMGAPI and DISM:
 *
 * \code
 *	wimlib_set_output_compression_type(wim, WIMLIB_COMPRESSION_TYPE_XPRESS);
 *	wimlib_set_output_chunk_size(wim, 4096);
 * \endcode
 *
 * However, "WIMBoot" also works with other XPRESS chunk sizes as well as LZX
 * with 32768 byte chunks.
 *
 * Note: ::WIMLIB_ADD_FLAG_WIMBOOT does something different from, and
 * independent from, ::WIMLIB_ADD_FLAG_BOOT.
 *
 * Since wimlib v1.8.3, ::WIMLIB_ADD_FLAG_WIMBOOT also causes offline WIM-backed
 * files to be added as the "real" files rather than as their reparse points,
 * provided that their data is already present in the WIM.  This feature can be
 * useful when updating a backing WIM file in an "offline" state.
 */
#define WIMLIB_ADD_FLAG_WIMBOOT			0x00001000

/**
 * If the add command involves adding a non-directory file to a location at
 * which there already exists a nondirectory file in the image, issue
 * ::WIMLIB_ERR_INVALID_OVERLAY instead of replacing the file.  This was the
 * default behavior before wimlib v1.7.0.
 */
#define WIMLIB_ADD_FLAG_NO_REPLACE		0x00002000

/**
 * Send ::WIMLIB_PROGRESS_MSG_TEST_FILE_EXCLUSION messages to the progress
 * function.
 *
 * Note: This method for file exclusions is independent from the capture
 * configuration file mechanism.
 */
#define WIMLIB_ADD_FLAG_TEST_FILE_EXCLUSION	0x00004000

/**
 * Since wimlib v1.9.0: create a temporary filesystem snapshot of the source
 * directory and add the files from it.  Currently, this option is only
 * supported on Windows, where it uses the Volume Shadow Copy Service (VSS).
 * Using this option, you can create a consistent backup of the system volume of
 * a running Windows system without running into problems with locked files.
 * For the VSS snapshot to be successfully created, your application must be run
 * as an Administrator, and it cannot be run in WoW64 mode (i.e. if Windows is
 * 64-bit, then your application must be 64-bit as well).
 */
#define WIMLIB_ADD_FLAG_SNAPSHOT		0x00008000

/**
 * Since wimlib v1.9.0: permit the library to discard file paths after the
 * initial scan.  If the application won't use
 * ::WIMLIB_WRITE_FLAG_SEND_DONE_WITH_FILE_MESSAGES while writing the WIM
 * archive, this flag can be used to allow the library to enable optimizations
 * such as opening files by inode number rather than by path.  Currently this
 * only makes a difference on Windows.
 */
#define WIMLIB_ADD_FLAG_FILE_PATHS_UNNEEDED	0x00010000

/** @} */
/** @addtogroup G_modifying_wims
 * @{ */

/** Do not issue an error if the path to delete does not exist. */
#define WIMLIB_DELETE_FLAG_FORCE			0x00000001

/** Delete the file or directory tree recursively; if not specified, an error is
 * issued if the path to delete is a directory. */
#define WIMLIB_DELETE_FLAG_RECURSIVE			0x00000002

/** @} */
/** @addtogroup G_modifying_wims
 * @{ */

/**
 * If a single image is being exported, mark it bootable in the destination WIM.
 * Alternatively, if ::WIMLIB_ALL_IMAGES is specified as the image to export,
 * the image in the source WIM (if any) that is marked as bootable is also
 * marked as bootable in the destination WIM.
 */
#define WIMLIB_EXPORT_FLAG_BOOT				0x00000001

/** Give the exported image(s) no names.  Avoids problems with image name
 * collisions.
 */
#define WIMLIB_EXPORT_FLAG_NO_NAMES			0x00000002

/** Give the exported image(s) no descriptions.  */
#define WIMLIB_EXPORT_FLAG_NO_DESCRIPTIONS		0x00000004

/** This advises the library that the program is finished with the source
 * WIMStruct and will not attempt to access it after the call to
 * wimlib_export_image(), with the exception of the call to wimlib_free().  */
#define WIMLIB_EXPORT_FLAG_GIFT				0x00000008

/**
 * Mark each exported image as WIMBoot-compatible.
 *
 * Note: by itself, this does change the destination WIM's compression type, nor
 * does it add the file @c \\Windows\\System32\\WimBootCompress.ini in the WIM
 * image.  Before writing the destination WIM, it's recommended to do something
 * like:
 *
 * \code
 *	wimlib_set_output_compression_type(wim, WIMLIB_COMPRESSION_TYPE_XPRESS);
 *	wimlib_set_output_chunk_size(wim, 4096);
 *	wimlib_add_tree(wim, image, L"myconfig.ini",
 *			L"\\Windows\\System32\\WimBootCompress.ini", 0);
 * \endcode
 */
#define WIMLIB_EXPORT_FLAG_WIMBOOT			0x00000010

/** @} */
/** @addtogroup G_extracting_wims
 * @{ */

/** Extract the image directly to an NTFS volume rather than a generic directory.
 * This mode is only available if wimlib was compiled with libntfs-3g support;
 * if not, ::WIMLIB_ERR_UNSUPPORTED will be returned.  In this mode, the
 * extraction target will be interpreted as the path to an NTFS volume image (as
 * a regular file or block device) rather than a directory.  It will be opened
 * using libntfs-3g, and the image will be extracted to the NTFS filesystem's
 * root directory.  Note: this flag cannot be used when wimlib_extract_image()
 * is called with ::WIMLIB_ALL_IMAGES as the @p image, nor can it be used with
 * wimlib_extract_paths() when passed multiple paths.  */
#define WIMLIB_EXTRACT_FLAG_NTFS			0x00000001

/** Since wimlib v1.13.4: Don't consider corrupted files to be an error.  Just
 * extract them in whatever form we can.  */
#define WIMLIB_EXTRACT_FLAG_RECOVER_DATA		0x00000002

/** UNIX-like systems only:  Extract UNIX-specific metadata captured with
 * ::WIMLIB_ADD_FLAG_UNIX_DATA.  */
#define WIMLIB_EXTRACT_FLAG_UNIX_DATA			0x00000020

/** Do not extract security descriptors.  This flag cannot be combined with
 * ::WIMLIB_EXTRACT_FLAG_STRICT_ACLS.  */
#define WIMLIB_EXTRACT_FLAG_NO_ACLS			0x00000040

/**
 * Fail immediately if the full security descriptor of any file or directory
 * cannot be set exactly as specified in the WIM image.  On Windows, the default
 * behavior without this flag when wimlib does not have permission to set the
 * correct security descriptor is to fall back to setting the security
 * descriptor with the SACL omitted, then with the DACL omitted, then with the
 * owner omitted, then not at all.  This flag cannot be combined with
 * ::WIMLIB_EXTRACT_FLAG_NO_ACLS.
 */
#define WIMLIB_EXTRACT_FLAG_STRICT_ACLS			0x00000080

/**
 * This is the extraction equivalent to ::WIMLIB_ADD_FLAG_RPFIX.  This forces
 * reparse-point fixups on, so absolute symbolic links or junction points will
 * be fixed to be absolute relative to the actual extraction root.  Reparse-
 * point fixups are done by default for wimlib_extract_image() and
 * wimlib_extract_image_from_pipe() if <c>WIM_HDR_FLAG_RP_FIX</c> is set in the
 * WIM header.  This flag cannot be combined with ::WIMLIB_EXTRACT_FLAG_NORPFIX.
 */
#define WIMLIB_EXTRACT_FLAG_RPFIX			0x00000100

/** Force reparse-point fixups on extraction off, regardless of the state of the
 * WIM_HDR_FLAG_RP_FIX flag in the WIM header.  This flag cannot be combined
 * with ::WIMLIB_EXTRACT_FLAG_RPFIX.  */
#define WIMLIB_EXTRACT_FLAG_NORPFIX			0x00000200

/** For wimlib_extract_paths() and wimlib_extract_pathlist() only:  Extract the
 * paths, each of which must name a regular file, to standard output.  */
#define WIMLIB_EXTRACT_FLAG_TO_STDOUT			0x00000400

/**
 * Instead of ignoring files and directories with names that cannot be
 * represented on the current platform (note: Windows has more restrictions on
 * filenames than POSIX-compliant systems), try to replace characters or append
 * junk to the names so that they can be extracted in some form.
 *
 * Note: this flag is unlikely to have any effect when extracting a WIM image
 * that was captured on Windows.
 */
#define WIMLIB_EXTRACT_FLAG_REPLACE_INVALID_FILENAMES	0x00000800

/**
 * On Windows, when there exist two or more files with the same case insensitive
 * name but different case sensitive names, try to extract them all by appending
 * junk to the end of them, rather than arbitrarily extracting only one.
 *
 * Note: this flag is unlikely to have any effect when extracting a WIM image
 * that was captured on Windows.
 */
#define WIMLIB_EXTRACT_FLAG_ALL_CASE_CONFLICTS		0x00001000

/** Do not ignore failure to set timestamps on extracted files.  This flag
 * currently only has an effect when extracting to a directory on UNIX-like
 * systems.  */
#define WIMLIB_EXTRACT_FLAG_STRICT_TIMESTAMPS		0x00002000

/** Do not ignore failure to set short names on extracted files.  This flag
 * currently only has an effect on Windows.  */
#define WIMLIB_EXTRACT_FLAG_STRICT_SHORT_NAMES          0x00004000

/** Do not ignore failure to extract symbolic links and junctions due to
 * permissions problems.  This flag currently only has an effect on Windows.  By
 * default, such failures are ignored since the default configuration of Windows
 * only allows the Administrator to create symbolic links.  */
#define WIMLIB_EXTRACT_FLAG_STRICT_SYMLINKS             0x00008000

/**
 * For wimlib_extract_paths() and wimlib_extract_pathlist() only:  Treat the
 * paths to extract as wildcard patterns ("globs") which may contain the
 * wildcard characters @c ? and @c *.  The @c ? character matches any
 * non-path-separator character, whereas the @c * character matches zero or more
 * non-path-separator characters.  Consequently, each glob may match zero or
 * more actual paths in the WIM image.
 *
 * By default, if a glob does not match any files, a warning but not an error
 * will be issued.  This is the case even if the glob did not actually contain
 * wildcard characters.  Use ::WIMLIB_EXTRACT_FLAG_STRICT_GLOB to get an error
 * instead.
 */
#define WIMLIB_EXTRACT_FLAG_GLOB_PATHS			0x00040000

/** In combination with ::WIMLIB_EXTRACT_FLAG_GLOB_PATHS, causes an error
 * (::WIMLIB_ERR_PATH_DOES_NOT_EXIST) rather than a warning to be issued when
 * one of the provided globs did not match a file.  */
#define WIMLIB_EXTRACT_FLAG_STRICT_GLOB			0x00080000

/**
 * Do not extract Windows file attributes such as readonly, hidden, etc.
 *
 * This flag has an effect on Windows as well as in the NTFS-3G extraction mode.
 */
#define WIMLIB_EXTRACT_FLAG_NO_ATTRIBUTES		0x00100000

/**
 * For wimlib_extract_paths() and wimlib_extract_pathlist() only:  Do not
 * preserve the directory structure of the archive when extracting --- that is,
 * place each extracted file or directory tree directly in the target directory.
 * The target directory will still be created if it does not already exist.
 */
#define WIMLIB_EXTRACT_FLAG_NO_PRESERVE_DIR_STRUCTURE	0x00200000

/**
 * Windows only: Extract files as "pointers" back to the WIM archive.
 *
 * The effects of this option are fairly complex.  See the documentation for the
 * <b>--wimboot</b> option of <b>wimapply</b> for more information.
 */
#define WIMLIB_EXTRACT_FLAG_WIMBOOT			0x00400000

/**
 * Since wimlib v1.8.2 and Windows-only: compress the extracted files using
 * System Compression, when possible.  This only works on either Windows 10 or
 * later, or on an older Windows to which Microsoft's wofadk.sys driver has been
 * added.  Several different compression formats may be used with System
 * Compression; this particular flag selects the XPRESS compression format with
 * 4096 byte chunks.
 */
#define WIMLIB_EXTRACT_FLAG_COMPACT_XPRESS4K		0x01000000

/** Like ::WIMLIB_EXTRACT_FLAG_COMPACT_XPRESS4K, but use XPRESS compression with
 * 8192 byte chunks.  */
#define WIMLIB_EXTRACT_FLAG_COMPACT_XPRESS8K		0x02000000

/** Like ::WIMLIB_EXTRACT_FLAG_COMPACT_XPRESS4K, but use XPRESS compression with
 * 16384 byte chunks.  */
#define WIMLIB_EXTRACT_FLAG_COMPACT_XPRESS16K		0x04000000

/** Like ::WIMLIB_EXTRACT_FLAG_COMPACT_XPRESS4K, but use LZX compression with
 * 32768 byte chunks.  */
#define WIMLIB_EXTRACT_FLAG_COMPACT_LZX			0x08000000

/** @} */
/** @addtogroup G_mounting_wim_images
 * @{ */

/** Mount the WIM image read-write rather than the default of read-only. */
#define WIMLIB_MOUNT_FLAG_READWRITE			0x00000001

/** Enable FUSE debugging by passing the @c -d option to @c fuse_main().  */
#define WIMLIB_MOUNT_FLAG_DEBUG				0x00000002

/** Do not allow accessing named data streams in the mounted WIM image.  */
#define WIMLIB_MOUNT_FLAG_STREAM_INTERFACE_NONE		0x00000004

/** Access named data streams in the mounted WIM image through extended file
 * attributes named "user.X", where X is the name of a data stream.  This is the
 * default mode.  */
#define WIMLIB_MOUNT_FLAG_STREAM_INTERFACE_XATTR	0x00000008

/** Access named data streams in the mounted WIM image by specifying the file
 * name, a colon, then the name of the data stream.  */
#define WIMLIB_MOUNT_FLAG_STREAM_INTERFACE_WINDOWS	0x00000010

/** Support UNIX owners, groups, modes, and special files.  */
#define WIMLIB_MOUNT_FLAG_UNIX_DATA			0x00000020

/** Allow other users to see the mounted filesystem.  This passes the @c
 * allow_other option to fuse_main().  */
#define WIMLIB_MOUNT_FLAG_ALLOW_OTHER			0x00000040

/** @} */
/** @addtogroup G_creating_and_opening_wims
 * @{ */

/** Verify the WIM contents against the WIM's integrity table, if present.  The
 * integrity table stores checksums for the raw data of the WIM file, divided
 * into fixed size chunks.  Verification will compute checksums and compare them
 * with the stored values.  If there are any mismatches, then
 * ::WIMLIB_ERR_INTEGRITY will be issued.  If the WIM file does not contain an
 * integrity table, then this flag has no effect.  */
#define WIMLIB_OPEN_FLAG_CHECK_INTEGRITY		0x00000001

/** Issue an error (::WIMLIB_ERR_IS_SPLIT_WIM) if the WIM is part of a split
 * WIM.  Software can provide this flag for convenience if it explicitly does
 * not want to support split WIMs.  */
#define WIMLIB_OPEN_FLAG_ERROR_IF_SPLIT			0x00000002

/** Check if the WIM is writable and issue an error
 * (::WIMLIB_ERR_WIM_IS_READONLY) if it is not.  A WIM is considered writable
 * only if it is writable at the filesystem level, does not have the
 * <c>WIM_HDR_FLAG_READONLY</c> flag set in its header, and is not part of a
 * spanned set.  It is not required to provide this flag before attempting to
 * make changes to the WIM, but with this flag you get an error immediately
 * rather than potentially much later, when wimlib_overwrite() is finally
 * called.  */
#define WIMLIB_OPEN_FLAG_WRITE_ACCESS			0x00000004

/** @} */
/** @addtogroup G_mounting_wim_images
 * @{ */

/** Provide ::WIMLIB_WRITE_FLAG_CHECK_INTEGRITY when committing the WIM image.
 * Ignored if ::WIMLIB_UNMOUNT_FLAG_COMMIT not also specified.  */
#define WIMLIB_UNMOUNT_FLAG_CHECK_INTEGRITY		0x00000001

/** Commit changes to the read-write mounted WIM image.
 * If this flag is not specified, changes will be discarded.  */
#define WIMLIB_UNMOUNT_FLAG_COMMIT			0x00000002

/** Provide ::WIMLIB_WRITE_FLAG_REBUILD when committing the WIM image.
 * Ignored if ::WIMLIB_UNMOUNT_FLAG_COMMIT not also specified.  */
#define WIMLIB_UNMOUNT_FLAG_REBUILD			0x00000004

/** Provide ::WIMLIB_WRITE_FLAG_RECOMPRESS when committing the WIM image.
 * Ignored if ::WIMLIB_UNMOUNT_FLAG_COMMIT not also specified.  */
#define WIMLIB_UNMOUNT_FLAG_RECOMPRESS			0x00000008

/**
 * In combination with ::WIMLIB_UNMOUNT_FLAG_COMMIT for a read-write mounted WIM
 * image, forces all file descriptors to the open WIM image to be closed before
 * committing it.
 *
 * Without ::WIMLIB_UNMOUNT_FLAG_COMMIT or with a read-only mounted WIM image,
 * this flag has no effect.
 */
#define WIMLIB_UNMOUNT_FLAG_FORCE			0x00000010

/** In combination with ::WIMLIB_UNMOUNT_FLAG_COMMIT for a read-write mounted
 * WIM image, causes the modified image to be committed to the WIM file as a
 * new, unnamed image appended to the archive.  The original image in the WIM
 * file will be unmodified.  */
#define WIMLIB_UNMOUNT_FLAG_NEW_IMAGE			0x00000020

/** @} */
/** @addtogroup G_modifying_wims
 * @{ */

/** Send ::WIMLIB_PROGRESS_MSG_UPDATE_BEGIN_COMMAND and
 * ::WIMLIB_PROGRESS_MSG_UPDATE_END_COMMAND messages.  */
#define WIMLIB_UPDATE_FLAG_SEND_PROGRESS		0x00000001

/** @} */
/** @addtogroup G_writing_and_overwriting_wims
 * @{ */

/**
 * Include an integrity table in the resulting WIM file.
 *
 * For ::WIMStruct's created with wimlib_open_wim(), the default behavior is to
 * include an integrity table if and only if one was present before.  For
 * ::WIMStruct's created with wimlib_create_new_wim(), the default behavior is
 * to not include an integrity table.
 */
#define WIMLIB_WRITE_FLAG_CHECK_INTEGRITY		0x00000001

/**
 * Do not include an integrity table in the resulting WIM file.  This is the
 * default behavior, unless the ::WIMStruct was created by opening a WIM with an
 * integrity table.
 */
#define WIMLIB_WRITE_FLAG_NO_CHECK_INTEGRITY		0x00000002

/**
 * Write the WIM as "pipable".  After writing a WIM with this flag specified,
 * images from it can be applied directly from a pipe using
 * wimlib_extract_image_from_pipe().  See the documentation for the
 * <b>--pipable</b> option of <b>wimcapture</b> for more information.  Beware:
 * WIMs written with this flag will not be compatible with Microsoft's software.
 *
 * For ::WIMStruct's created with wimlib_open_wim(), the default behavior is to
 * write the WIM as pipable if and only if it was pipable before.  For
 * ::WIMStruct's created with wimlib_create_new_wim(), the default behavior is
 * to write the WIM as non-pipable.
 */
#define WIMLIB_WRITE_FLAG_PIPABLE			0x00000004

/**
 * Do not write the WIM as "pipable".  This is the default behavior, unless the
 * ::WIMStruct was created by opening a pipable WIM.
 */
#define WIMLIB_WRITE_FLAG_NOT_PIPABLE			0x00000008

/**
 * When writing data to the WIM file, recompress it, even if the data is already
 * available in the desired compressed form (for example, in a WIM file from
 * which an image has been exported using wimlib_export_image()).
 *
 * ::WIMLIB_WRITE_FLAG_RECOMPRESS can be used to recompress with a higher
 * compression ratio for the same compression type and chunk size.  Simply using
 * the default compression settings may suffice for this, especially if the WIM
 * file was created using another program/library that may not use as
 * sophisticated compression algorithms.  Or,
 * wimlib_set_default_compression_level() can be called beforehand to set an
 * even higher compression level than the default.
 *
 * If the WIM contains solid resources, then ::WIMLIB_WRITE_FLAG_RECOMPRESS can
 * be used in combination with ::WIMLIB_WRITE_FLAG_SOLID to prevent any solid
 * resources from being re-used.  Otherwise, solid resources are re-used
 * somewhat more liberally than normal compressed resources.
 *
 * ::WIMLIB_WRITE_FLAG_RECOMPRESS does <b>not</b> cause recompression of data
 * that would not otherwise be written.  For example, a call to
 * wimlib_overwrite() with ::WIMLIB_WRITE_FLAG_RECOMPRESS will not, by itself,
 * cause already-existing data in the WIM file to be recompressed.  To force the
 * WIM file to be fully rebuilt and recompressed, combine
 * ::WIMLIB_WRITE_FLAG_RECOMPRESS with ::WIMLIB_WRITE_FLAG_REBUILD.
 */
#define WIMLIB_WRITE_FLAG_RECOMPRESS			0x00000010

/**
 * Immediately before closing the WIM file, sync its data to disk.
 *
 * This flag forces the function to wait until the data is safely on disk before
 * returning success.  Otherwise, modern operating systems tend to cache data
 * for some time (in some cases, 30+ seconds) before actually writing it to
 * disk, even after reporting to the application that the writes have succeeded.
 *
 * wimlib_overwrite() will set this flag automatically if it decides to
 * overwrite the WIM file via a temporary file instead of in-place.  This is
 * necessary on POSIX systems; it will, for example, avoid problems with delayed
 * allocation on ext4.
 */
#define WIMLIB_WRITE_FLAG_FSYNC				0x00000020

/**
 * For wimlib_overwrite(): rebuild the entire WIM file, even if it otherwise
 * could be updated in-place by appending to it.  Any data that existed in the
 * original WIM file but is not actually needed by any of the remaining images
 * will not be included.  This can free up space left over after previous
 * in-place modifications to the WIM file.
 *
 * This flag can be combined with ::WIMLIB_WRITE_FLAG_RECOMPRESS to force all
 * data to be recompressed.  Otherwise, compressed data is re-used if possible.
 *
 * wimlib_write() ignores this flag.
 */
#define WIMLIB_WRITE_FLAG_REBUILD			0x00000040

/**
 * For wimlib_overwrite(): override the default behavior after one or more calls
 * to wimlib_delete_image(), which is to rebuild the entire WIM file.  With this
 * flag, only minimal changes to correctly remove the image from the WIM file
 * will be taken.  This can be much faster, but it will result in the WIM file
 * getting larger rather than smaller.
 *
 * wimlib_write() ignores this flag.
 */
#define WIMLIB_WRITE_FLAG_SOFT_DELETE			0x00000080

/**
 * For wimlib_overwrite(), allow overwriting the WIM file even if the readonly
 * flag (<c>WIM_HDR_FLAG_READONLY</c>) is set in the WIM header.  This can be
 * used following a call to wimlib_set_wim_info() with the
 * ::WIMLIB_CHANGE_READONLY_FLAG flag to actually set the readonly flag on the
 * on-disk WIM file.
 *
 * wimlib_write() ignores this flag.
 */
#define WIMLIB_WRITE_FLAG_IGNORE_READONLY_FLAG		0x00000100

/**
 * Do not include file data already present in other WIMs.  This flag can be
 * used to write a "delta" WIM after the WIM files on which the delta is to be
 * based were referenced with wimlib_reference_resource_files() or
 * wimlib_reference_resources().
 */
#define WIMLIB_WRITE_FLAG_SKIP_EXTERNAL_WIMS		0x00000200

/** Deprecated; this flag should not be used outside of the library itself.  */
#define WIMLIB_WRITE_FLAG_STREAMS_OK			0x00000400

/**
 * For wimlib_write(), retain the WIM's GUID instead of generating a new one.
 *
 * wimlib_overwrite() sets this by default, since the WIM remains, logically,
 * the same file.
 */
#define WIMLIB_WRITE_FLAG_RETAIN_GUID			0x00000800

/**
 * Concatenate files and compress them together, rather than compress each file
 * independently.  This is also known as creating a "solid archive".  This tends
 * to produce a better compression ratio at the cost of much slower random
 * access.
 *
 * WIM files created with this flag are only compatible with wimlib v1.6.0 or
 * later, WIMGAPI Windows 8 or later, and DISM Windows 8.1 or later.  WIM files
 * created with this flag use a different version number in their header (3584
 * instead of 68864) and are also called "ESD files".
 *
 * Note that providing this flag does not affect the "append by default"
 * behavior of wimlib_overwrite().  In other words, wimlib_overwrite() with just
 * ::WIMLIB_WRITE_FLAG_SOLID can be used to append solid-compressed data to a
 * WIM file that originally did not contain any solid-compressed data.  But if
 * you instead want to rebuild and recompress an entire WIM file in solid mode,
 * then also provide ::WIMLIB_WRITE_FLAG_REBUILD and
 * ::WIMLIB_WRITE_FLAG_RECOMPRESS.
 *
 * Currently, new solid resources will, by default, be written using LZMS
 * compression with 64 MiB (67108864 byte) chunks.  Use
 * wimlib_set_output_pack_compression_type() and/or
 * wimlib_set_output_pack_chunk_size() to change this.  This is independent of
 * the WIM's main compression type and chunk size; you can have a WIM that
 * nominally uses LZX compression and 32768 byte chunks but actually contains
 * LZMS-compressed solid resources, for example.  However, if including solid
 * resources, I suggest that you set the WIM's main compression type to LZMS as
 * well, either by creating the WIM with
 * ::wimlib_create_new_wim(::WIMLIB_COMPRESSION_TYPE_LZMS, ...) or by calling
 * ::wimlib_set_output_compression_type(..., ::WIMLIB_COMPRESSION_TYPE_LZMS).
 *
 * This flag will be set by default when writing or overwriting a WIM file that
 * either already contains solid resources, or has had solid resources exported
 * into it and the WIM's main compression type is LZMS.
 */
#define WIMLIB_WRITE_FLAG_SOLID				0x00001000

/**
 * Send ::WIMLIB_PROGRESS_MSG_DONE_WITH_FILE messages while writing the WIM
 * file.  This is only needed in the unusual case that the library user needs to
 * know exactly when wimlib has read each file for the last time.
 */
#define WIMLIB_WRITE_FLAG_SEND_DONE_WITH_FILE_MESSAGES	0x00002000

/**
 * Do not consider content similarity when arranging file data for solid
 * compression.  Providing this flag will typically worsen the compression
 * ratio, so only provide this flag if you know what you are doing.
 */
#define WIMLIB_WRITE_FLAG_NO_SOLID_SORT			0x00004000

/**
 * Since wimlib v1.8.3 and for wimlib_overwrite() only: <b>unsafely</b> compact
 * the WIM file in-place, without appending.  Existing resources are shifted
 * down to fill holes and new resources are appended as needed.  The WIM file is
 * truncated to its final size, which may shrink the on-disk file.  <b>This
 * operation cannot be safely interrupted.  If the operation is interrupted,
 * then the WIM file will be corrupted, and it may be impossible (or at least
 * very difficult) to recover any data from it.  Users of this flag are expected
 * to know what they are doing and assume responsibility for any data corruption
 * that may result.</b>
 *
 * If the WIM file cannot be compacted in-place because of its structure, its
 * layout, or other requested write parameters, then wimlib_overwrite() fails
 * with ::WIMLIB_ERR_COMPACTION_NOT_POSSIBLE, and the caller may wish to retry
 * the operation without this flag.
 */
#define WIMLIB_WRITE_FLAG_UNSAFE_COMPACT		0x00008000

/** @} */
/** @addtogroup G_general
 * @{ */

/** Deprecated; no longer has any effect.  */
#define WIMLIB_INIT_FLAG_ASSUME_UTF8			0x00000001

/** Windows-only: do not attempt to acquire additional privileges (currently
 * SeBackupPrivilege, SeRestorePrivilege, SeSecurityPrivilege,
 * SeTakeOwnershipPrivilege, and SeManageVolumePrivilege) when initializing the
 * library.  This flag is intended for the case where the calling program
 * manages these privileges itself.  Note: by default, no error is issued if
 * privileges cannot be acquired, although related errors may be reported later,
 * depending on if the operations performed actually require additional
 * privileges or not.  */
#define WIMLIB_INIT_FLAG_DONT_ACQUIRE_PRIVILEGES	0x00000002

/** Windows only:  If ::WIMLIB_INIT_FLAG_DONT_ACQUIRE_PRIVILEGES not specified,
 * return ::WIMLIB_ERR_INSUFFICIENT_PRIVILEGES if privileges that may be needed
 * to read all possible data and metadata for a capture operation could not be
 * acquired.  Can be combined with ::WIMLIB_INIT_FLAG_STRICT_APPLY_PRIVILEGES.
 */
#define WIMLIB_INIT_FLAG_STRICT_CAPTURE_PRIVILEGES	0x00000004

/** Windows only:  If ::WIMLIB_INIT_FLAG_DONT_ACQUIRE_PRIVILEGES not specified,
 * return ::WIMLIB_ERR_INSUFFICIENT_PRIVILEGES if privileges that may be needed
 * to restore all possible data and metadata for an apply operation could not be
 * acquired.  Can be combined with ::WIMLIB_INIT_FLAG_STRICT_CAPTURE_PRIVILEGES.
 */
#define WIMLIB_INIT_FLAG_STRICT_APPLY_PRIVILEGES	0x00000008

/** Default to interpreting WIM paths case sensitively (default on UNIX-like
 * systems).  */
#define WIMLIB_INIT_FLAG_DEFAULT_CASE_SENSITIVE		0x00000010

/** Default to interpreting WIM paths case insensitively (default on Windows).
 * This does not apply to mounted images.  */
#define WIMLIB_INIT_FLAG_DEFAULT_CASE_INSENSITIVE	0x00000020

/** @} */
/** @addtogroup G_nonstandalone_wims
 * @{ */

/** For wimlib_reference_resource_files(), enable shell-style filename globbing.
 * Ignored by wimlib_reference_resources().  */
#define WIMLIB_REF_FLAG_GLOB_ENABLE		0x00000001

/** For wimlib_reference_resource_files(), issue an error
 * (::WIMLIB_ERR_GLOB_HAD_NO_MATCHES) if a glob did not match any files.  The
 * default behavior without this flag is to issue no error at that point, but
 * then attempt to open the glob as a literal path, which of course will fail
 * anyway if no file exists at that path.  No effect if
 * ::WIMLIB_REF_FLAG_GLOB_ENABLE is not also specified.  Ignored by
 * wimlib_reference_resources().  */
#define WIMLIB_REF_FLAG_GLOB_ERR_ON_NOMATCH	0x00000002

/** @} */
/** @addtogroup G_modifying_wims
 * @{ */

/** The specific type of update to perform. */
enum wimlib_update_op {
	/** Add a new file or directory tree to the image.  */
	WIMLIB_UPDATE_OP_ADD = 0,

	/** Delete a file or directory tree from the image.  */
	WIMLIB_UPDATE_OP_DELETE = 1,

	/** Rename a file or directory tree in the image.  */
	WIMLIB_UPDATE_OP_RENAME = 2,
};

/** Data for a ::WIMLIB_UPDATE_OP_ADD operation. */
struct wimlib_add_command {
	/** Filesystem path to the file or directory tree to add.  */
	wimlib_tchar *fs_source_path;

	/** Destination path in the image.  To specify the root directory of the
	 * image, use ::WIMLIB_WIM_ROOT_PATH.  */
	wimlib_tchar *wim_target_path;

	/** Path to capture configuration file to use, or @c NULL if not
	 * specified.  */
	wimlib_tchar *config_file;

	/** Bitwise OR of WIMLIB_ADD_FLAG_* flags. */
	int add_flags;
};

/** Data for a ::WIMLIB_UPDATE_OP_DELETE operation. */
struct wimlib_delete_command {

	/** The path to the file or directory within the image to delete.  */
	wimlib_tchar *wim_path;

	/** Bitwise OR of WIMLIB_DELETE_FLAG_* flags.  */
	int delete_flags;
};

/** Data for a ::WIMLIB_UPDATE_OP_RENAME operation. */
struct wimlib_rename_command {

	/** The path to the source file or directory within the image.  */
	wimlib_tchar *wim_source_path;

	/** The path to the destination file or directory within the image.  */
	wimlib_tchar *wim_target_path;

	/** Reserved; set to 0.  */
	int rename_flags;
};

/** Specification of an update to perform on a WIM image. */
struct wimlib_update_command {

	enum wimlib_update_op op;

	union {
		struct wimlib_add_command add;
		struct wimlib_delete_command delete_; /* Underscore is for C++
							 compatibility.  */
		struct wimlib_rename_command rename;
	};
};

/** @} */
/** @addtogroup G_general
 * @{ */

/**
 * Possible values of the error code returned by many functions in wimlib.
 *
 * See the documentation for each wimlib function to see specifically what error
 * codes can be returned by a given function, and what they mean.
 */
enum wimlib_error_code {
	WIMLIB_ERR_SUCCESS                            = 0,
	WIMLIB_ERR_ALREADY_LOCKED                     = 1,
	WIMLIB_ERR_DECOMPRESSION                      = 2,
	WIMLIB_ERR_FUSE                               = 6,
	WIMLIB_ERR_GLOB_HAD_NO_MATCHES                = 8,
	WIMLIB_ERR_IMAGE_COUNT                        = 10,
	WIMLIB_ERR_IMAGE_NAME_COLLISION               = 11,
	WIMLIB_ERR_INSUFFICIENT_PRIVILEGES            = 12,
	WIMLIB_ERR_INTEGRITY                          = 13,
	WIMLIB_ERR_INVALID_CAPTURE_CONFIG             = 14,
	WIMLIB_ERR_INVALID_CHUNK_SIZE                 = 15,
	WIMLIB_ERR_INVALID_COMPRESSION_TYPE           = 16,
	WIMLIB_ERR_INVALID_HEADER                     = 17,
	WIMLIB_ERR_INVALID_IMAGE                      = 18,
	WIMLIB_ERR_INVALID_INTEGRITY_TABLE            = 19,
	WIMLIB_ERR_INVALID_LOOKUP_TABLE_ENTRY         = 20,
	WIMLIB_ERR_INVALID_METADATA_RESOURCE          = 21,
	WIMLIB_ERR_INVALID_OVERLAY                    = 23,
	WIMLIB_ERR_INVALID_PARAM                      = 24,
	WIMLIB_ERR_INVALID_PART_NUMBER                = 25,
	WIMLIB_ERR_INVALID_PIPABLE_WIM                = 26,
	WIMLIB_ERR_INVALID_REPARSE_DATA               = 27,
	WIMLIB_ERR_INVALID_RESOURCE_HASH              = 28,
	WIMLIB_ERR_INVALID_UTF16_STRING               = 30,
	WIMLIB_ERR_INVALID_UTF8_STRING                = 31,
	WIMLIB_ERR_IS_DIRECTORY                       = 32,
	WIMLIB_ERR_IS_SPLIT_WIM                       = 33,
	WIMLIB_ERR_LINK                               = 35,
	WIMLIB_ERR_METADATA_NOT_FOUND                 = 36,
	WIMLIB_ERR_MKDIR                              = 37,
	WIMLIB_ERR_MQUEUE                             = 38,
	WIMLIB_ERR_NOMEM                              = 39,
	WIMLIB_ERR_NOTDIR                             = 40,
	WIMLIB_ERR_NOTEMPTY                           = 41,
	WIMLIB_ERR_NOT_A_REGULAR_FILE                 = 42,
	WIMLIB_ERR_NOT_A_WIM_FILE                     = 43,
	WIMLIB_ERR_NOT_PIPABLE                        = 44,
	WIMLIB_ERR_NO_FILENAME                        = 45,
	WIMLIB_ERR_NTFS_3G                            = 46,
	WIMLIB_ERR_OPEN                               = 47,
	WIMLIB_ERR_OPENDIR                            = 48,
	WIMLIB_ERR_PATH_DOES_NOT_EXIST                = 49,
	WIMLIB_ERR_READ                               = 50,
	WIMLIB_ERR_READLINK                           = 51,
	WIMLIB_ERR_RENAME                             = 52,
	WIMLIB_ERR_REPARSE_POINT_FIXUP_FAILED         = 54,
	WIMLIB_ERR_RESOURCE_NOT_FOUND                 = 55,
	WIMLIB_ERR_RESOURCE_ORDER                     = 56,
	WIMLIB_ERR_SET_ATTRIBUTES                     = 57,
	WIMLIB_ERR_SET_REPARSE_DATA                   = 58,
	WIMLIB_ERR_SET_SECURITY                       = 59,
	WIMLIB_ERR_SET_SHORT_NAME                     = 60,
	WIMLIB_ERR_SET_TIMESTAMPS                     = 61,
	WIMLIB_ERR_SPLIT_INVALID                      = 62,
	WIMLIB_ERR_STAT                               = 63,
	WIMLIB_ERR_UNEXPECTED_END_OF_FILE             = 65,
	WIMLIB_ERR_UNICODE_STRING_NOT_REPRESENTABLE   = 66,
	WIMLIB_ERR_UNKNOWN_VERSION                    = 67,
	WIMLIB_ERR_UNSUPPORTED                        = 68,
	WIMLIB_ERR_UNSUPPORTED_FILE                   = 69,
	WIMLIB_ERR_WIM_IS_READONLY                    = 71,
	WIMLIB_ERR_WRITE                              = 72,
	WIMLIB_ERR_XML                                = 73,
	WIMLIB_ERR_WIM_IS_ENCRYPTED                   = 74,
	WIMLIB_ERR_WIMBOOT                            = 75,
	WIMLIB_ERR_ABORTED_BY_PROGRESS                = 76,
	WIMLIB_ERR_UNKNOWN_PROGRESS_STATUS            = 77,
	WIMLIB_ERR_MKNOD                              = 78,
	WIMLIB_ERR_MOUNTED_IMAGE_IS_BUSY              = 79,
	WIMLIB_ERR_NOT_A_MOUNTPOINT                   = 80,
	WIMLIB_ERR_NOT_PERMITTED_TO_UNMOUNT           = 81,
	WIMLIB_ERR_FVE_LOCKED_VOLUME                  = 82,
	WIMLIB_ERR_UNABLE_TO_READ_CAPTURE_CONFIG      = 83,
	WIMLIB_ERR_WIM_IS_INCOMPLETE                  = 84,
	WIMLIB_ERR_COMPACTION_NOT_POSSIBLE            = 85,
	WIMLIB_ERR_IMAGE_HAS_MULTIPLE_REFERENCES      = 86,
	WIMLIB_ERR_DUPLICATE_EXPORTED_IMAGE           = 87,
	WIMLIB_ERR_CONCURRENT_MODIFICATION_DETECTED   = 88,
	WIMLIB_ERR_SNAPSHOT_FAILURE                   = 89,
	WIMLIB_ERR_INVALID_XATTR                      = 90,
	WIMLIB_ERR_SET_XATTR                          = 91,
};


/** Used to indicate no image or an invalid image. */
#define WIMLIB_NO_IMAGE		0

/** Used to specify all images in the WIM. */
#define WIMLIB_ALL_IMAGES	(-1)

/** @}  */
```

## Compression-only constant

```c
#define WIMLIB_COMPRESSOR_FLAG_DESTRUCTIVE 0x80000000
```

This high bit is combined with the compression level, not a WIM write flag.
See the create-compressor contract above for when input data may be destroyed.

## Coverage and provenance

This document contains **72 exported functions**. The header SHA-256 is
`3769368886f5efc0e62ae2ecd7270697c57e21639b35808f70def87cec9dadc6`. Public type and flag coverage
includes all header declarations preceding the function section, plus the
compression-only destructive flag defined within the function section. A future
header update must regenerate/audit the function set, constants, layouts and
contracts together.
