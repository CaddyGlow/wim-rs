// SPDX-License-Identifier: LGPL-2.1-or-later
// String tables translated from wimlib error.c/wim.c, cd5e231c348c255ae5088873b5a66ee0eb96fa07.
//! Low-level WIM archive implementation and compatibility entry points.

#[cfg(any(windows, test, feature = "disk-capture"))]
mod ntfs_metadata;
#[cfg(windows)]
mod windows_ntfs;
/// Raw file XML access through host C allocation and stdio.
pub mod xml_data;
pub use xml_data::*;

/// Native directory-entry callback traversal.
pub mod dir_tree;
pub use dir_tree::*;

/// Native metadata and blob verification.
pub mod verify;
pub use verify::*;

/// Native resource listing callbacks.
pub(crate) mod blob_index;
pub mod lookup;
pub use lookup::*;

/// Native compressor ownership and block encoding exports.
pub mod compress;
/// Native opaque decompressor ownership and block decoding exports.
pub mod decompress;
/// Independently owned image export with lazy source snapshots.
pub mod export;
/// Native archive handle lifetime and file opening.
pub mod handles;
pub use export::*;
/// Native filesystem extraction from retained image metadata and resources.
pub mod extract;
pub use extract::*;
/// Deferred filesystem capture plans and captured stream ownership.
pub mod capture;
pub use capture::*;
/// Owned pending images and native image deletion.
pub mod image_mutation;
pub use image_mutation::*;
/// Transactional path deletion and renaming.
pub mod path_mutation;
pub use path_mutation::*;
/// Journaled multi-command image updates and command ABI.
pub mod update;
pub use update::*;
/// Header information and output settings exposed through the C ABI.
pub mod info;
/// Compression memory estimates and default levels.
pub mod memory;
/// Image XML properties exposed through the C ABI.
pub mod properties;
pub use compress::*;
pub use decompress::*;
pub use handles::*;
pub use info::*;
/// Buffered writing from current native WIM handles.
pub mod write;
pub use memory::*;
pub use properties::*;
pub use write::*;
/// Native overwrite policy and commit lifecycle.
pub mod overwrite;
pub use overwrite::*;
/// Split-set file writing and joining through native resource readers.
pub mod split_join;
pub use split_join::*;

/// Original progress callback layout and borrowed registration lifecycle.
pub mod progress;
pub use progress::*;

/// Text-file loading and original platform encoding detection.
pub mod text_file;
pub use text_file::*;

/// Lazy data resources retained from independently owned WIM sources.
pub mod references;
pub use references::*;
pub mod mount;
pub use mount::*;
pub mod template;
pub use template::*;
/// Optional upstream test helpers; these do not enable randomized capture until implemented.
#[cfg(feature = "test-support")]
pub mod test_support;

/// Original diagnostic sink ownership and enabled error reporting.
pub mod diagnostics;
pub use diagnostics::*;
/// Explicit and automatic runtime initialization lifecycle.
pub mod runtime;
pub use runtime::*;
/// Host C allocation for externally owned return buffers.
pub(crate) mod allocation;

/// Deprecated image and debugging header output through host C stdio.
#[cfg(any(unix, windows))]
pub mod print;
#[cfg(any(unix, windows))]
pub use print::*;

/// Platform character used by the unchanged wimlib header.
#[cfg(not(windows))]
pub type TChar = std::ffi::c_char;
/// Platform character used by the unchanged wimlib header.
#[cfg(windows)]
pub type TChar = u16;

/// Original 1.14.5 ABI baseline version encoded in three bit fields.
#[unsafe(no_mangle)]
pub extern "C" fn wimlib_get_version() -> u32 {
    (1 << 20) | (14 << 10) | 5
}

const fn platform_text<const N: usize>(bytes: &[u8]) -> [TChar; N] {
    let mut output = [0; N];
    let mut index = 0;
    while index < N {
        output[index] = bytes[index] as TChar;
        index += 1;
    }
    output
}
macro_rules! text_pointer {
    ($bytes:expr) => {{
        static TEXT: [TChar; $bytes.len()] = platform_text($bytes);
        TEXT.as_ptr()
    }};
}
/// Static version text for the ABI baseline implemented by this candidate.
#[unsafe(no_mangle)]
pub extern "C" fn wimlib_get_version_string() -> *const TChar {
    text_pointer!(b"1.14.5\0")
}
/// Static compression name, or `Invalid` for an unrecognized integer.
#[unsafe(no_mangle)]
pub extern "C" fn wimlib_get_compression_type_string(code: std::ffi::c_int) -> *const TChar {
    match code {
        0 => text_pointer!(b"None\0"),
        1 => text_pointer!(b"XPRESS\0"),
        2 => text_pointer!(b"LZX\0"),
        3 => text_pointer!(b"LZMS\0"),
        _ => text_pointer!(b"Invalid\0"),
    }
}
macro_rules! error_text {
    ($message:literal) => {{
        const BYTES: &[u8] = concat!($message, "\0").as_bytes();
        static TEXT: [TChar; BYTES.len()] = platform_text(BYTES);
        ($message, TEXT.as_slice())
    }};
}

/// Static error message, or `Unknown error` for unused/out-of-range values.
#[unsafe(no_mangle)]
pub extern "C" fn wimlib_get_error_string(code: std::ffi::c_int) -> *const TChar {
    error_text(code).1.as_ptr()
}

pub(crate) fn error_message(code: i32) -> &'static str {
    error_text(code).0
}

fn error_text(code: i32) -> (&'static str, &'static [TChar]) {
    match code {
        #[cfg(feature = "test-support")]
        200 => error_text!("A difference was detected between the two images being compared"),
        0 => error_text!("Success"),
        30 => error_text!("A string was not a valid UTF-16 string"),
        31 => error_text!("A string was not a valid UTF-8 string"),
        46 => error_text!("NTFS-3G encountered an error (check errno)"),
        1 => error_text!("The WIM is already locked for writing"),
        2 => error_text!("The WIM contains invalid compressed data"),
        6 => error_text!("An error was returned by fuse_main()"),
        8 => error_text!("The provided file glob did not match any files"),
        10 => error_text!(
            "Inconsistent image count among the metadata resources, the WIM header, and/or the XML data"
        ),
        11 => error_text!("Tried to add an image with a name that is already in use"),
        12 => error_text!("The user does not have sufficient privileges"),
        13 => error_text!("The WIM file is corrupted (failed integrity check)"),
        14 => error_text!("The contents of the capture configuration file were invalid"),
        15 => error_text!("The compression chunk size was unrecognized"),
        16 => error_text!("The compression type was unrecognized"),
        17 => error_text!("The WIM header was invalid"),
        18 => error_text!("Tried to select an image that does not exist in the WIM"),
        19 => error_text!("The WIM's integrity table is invalid"),
        20 => error_text!("An entry in the WIM's lookup table is invalid"),
        21 => error_text!("The metadata resource is invalid"),
        23 => error_text!("Conflicting files in overlay when creating a WIM image"),
        24 => error_text!("An invalid parameter was given"),
        25 => error_text!("The part number or total parts of the WIM is invalid"),
        26 => error_text!("The pipable WIM is invalid"),
        27 => error_text!("The reparse data of a reparse point was invalid"),
        28 => error_text!(
            "The SHA-1 message digest of a WIM resource did not match the expected value"
        ),
        32 => error_text!("One of the specified paths to delete was a directory"),
        33 => {
            error_text!("The WIM is part of a split WIM, which is not supported for this operation")
        }
        35 => error_text!(
            "Failed to create a hard or symbolic link when extracting a file from the WIM"
        ),
        36 => error_text!("The WIM does not contain image metadata; it only contains file data"),
        37 => error_text!("Failed to create a directory"),
        38 => error_text!("Failed to create or use a POSIX message queue"),
        39 => error_text!("Ran out of memory"),
        40 => error_text!("Expected a directory"),
        41 => error_text!("Directory was not empty"),
        42 => error_text!(
            "One of the specified paths to extract did not correspond to a regular file"
        ),
        43 => {
            error_text!("The file did not begin with the magic characters that identify a WIM file")
        }
        45 => error_text!("The WIM is not identified with a filename"),
        44 => error_text!("The WIM was not captured such that it can be applied from a pipe"),
        47 => error_text!("Failed to open a file"),
        48 => error_text!("Failed to open a directory"),
        49 => error_text!("The path does not exist in the WIM image"),
        50 => error_text!("Could not read data from a file"),
        51 => error_text!("Could not read the target of a symbolic link"),
        52 => error_text!("Could not rename a file"),
        54 => error_text!("Unable to complete reparse point fixup"),
        55 => {
            error_text!("A file resource needed to complete the operation was missing from the WIM")
        }
        56 => error_text!("The components of the WIM were arranged in an unexpected order"),
        57 => error_text!("Failed to set attributes on extracted file"),
        58 => error_text!("Failed to set reparse data on extracted file"),
        59 => {
            error_text!("Failed to set file owner, group, or other permissions on extracted file")
        }
        60 => error_text!("Failed to set short name on extracted file"),
        61 => error_text!("Failed to set timestamps on extracted file"),
        62 => error_text!("The WIM is part of an invalid split WIM"),
        63 => error_text!("Could not read the metadata for a file or directory"),
        65 => error_text!("Unexpectedly reached the end of the file"),
        66 => error_text!(
            "A Unicode string could not be represented in the current locale's encoding"
        ),
        67 => error_text!("The WIM file is marked with an unknown version number"),
        68 => error_text!("The requested operation is unsupported"),
        69 => error_text!("A file in the directory tree to archive was not of a supported type"),
        71 => error_text!("The WIM is read-only (file permissions, header flag, or split WIM)"),
        72 => error_text!("Failed to write data to a file"),
        73 => error_text!("The XML data of the WIM is invalid"),
        74 => error_text!("The WIM file (or parts of it) is encrypted"),
        75 => error_text!("Failed to set WIMBoot pointer data"),
        76 => error_text!("The operation was aborted by the library user"),
        77 => error_text!("The user-provided progress function returned an unrecognized value"),
        78 => error_text!("Unable to create a special file (e.g. device node or socket)"),
        79 => error_text!("There are still files open on the mounted WIM image"),
        80 => error_text!("There is not a WIM image mounted on the directory"),
        81 => error_text!("The current user does not have permission to unmount the WIM image"),
        82 => error_text!("The volume must be unlocked before it can be used"),
        83 => error_text!("The capture configuration file could not be read"),
        84 => error_text!("The WIM file is incomplete"),
        85 => error_text!(
            "The WIM file cannot be compacted because of its format, its layout, or the write parameters specified by the user"
        ),
        86 => error_text!(
            "The WIM image cannot be modified because it is currently referenced from multiple places"
        ),
        87 => error_text!("The destination WIM already contains one of the source images"),
        88 => error_text!("A file being added to a WIM image was concurrently modified"),
        89 => error_text!("Unable to create a filesystem snapshot"),
        90 => error_text!("An extended attribute entry in the WIM image is invalid"),
        91 => error_text!("Failed to set an extended attribute on an extracted file"),
        _ => error_text!("Unknown error"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn version_uses_original_bit_layout() {
        assert_eq!(wimlib_get_version(), (1 << 20) | (14 << 10) | 5);
    }
    #[cfg(not(windows))]
    #[test]
    fn static_strings_preserve_lifetime_and_sparse_error_values() {
        let saved = wimlib_get_error_string(30);
        for code in -2..=202 {
            assert!(!wimlib_get_error_string(code).is_null());
        }
        // SAFETY: Every export returns immutable NUL-terminated static storage.
        assert_eq!(
            unsafe { std::ffi::CStr::from_ptr(saved) }.to_bytes(),
            b"A string was not a valid UTF-16 string"
        );
        // SAFETY: As above, the pointer is valid for the process lifetime.
        assert_eq!(
            unsafe { std::ffi::CStr::from_ptr(wimlib_get_error_string(-1)) }.to_bytes(),
            b"Unknown error"
        );
        // SAFETY: As above, the pointer is valid for the process lifetime.
        let test_message =
            unsafe { std::ffi::CStr::from_ptr(wimlib_get_error_string(200)) }.to_bytes();
        assert_eq!(
            test_message,
            if cfg!(feature = "test-support") {
                b"A difference was detected between the two images being compared".as_slice()
            } else {
                b"Unknown error".as_slice()
            }
        );
    }
}

mod backing;

mod collections;
