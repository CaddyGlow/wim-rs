// SPDX-License-Identifier: LGPL-2.1-or-later
//! Original debug/information output through the host C stdout stream.
use crate::engine::handles::WimHandle;
use std::ffi::{CString, c_int};
use wim_format::{Header, ResourceHeader, xml::XmlInfo};

#[cfg(unix)]
unsafe extern "C" {
    static mut stdout: *mut libc::FILE;
}
#[cfg(windows)]
unsafe extern "C" {
    fn wprintf(format: *const u16, ...) -> c_int;
    fn wcsftime(output: *mut u16, size: usize, format: *const u16, time: *const libc::tm) -> usize;
}

// Preserve each original formatting argument boundary. Windows text-mode
// wprintf can stop converting one %ls argument while still emitting its suffix.
struct PrintField {
    prefix: String,
    value: Vec<u8>,
    suffix: &'static str,
}
#[derive(Default)]
struct ImageOutput {
    fields: Vec<PrintField>,
}
impl ImageOutput {
    fn text(&mut self, value: &[u8]) {
        self.fields.push(PrintField {
            prefix: String::new(),
            value: value.to_vec(),
            suffix: "",
        });
    }
    fn field(&mut self, prefix: String, value: &[u8], suffix: &'static str) {
        self.fields.push(PrintField {
            prefix,
            value: value.to_vec(),
            suffix,
        });
    }
}
fn line(output: &mut Vec<u8>, label: &str, value: impl std::fmt::Display) {
    output.extend_from_slice(format!("{label:<28}= {value}\n").as_bytes());
}
fn resource(output: &mut Vec<u8>, names: [&str; 4], header: &ResourceHeader) {
    line(output, names[0], header.size_in_wim);
    line(output, names[1], format_args!("0x{:x}", header.flags));
    line(output, names[2], header.offset_in_wim);
    line(output, names[3], header.uncompressed_size);
}
fn header_text(header: &Header) -> Vec<u8> {
    let mut output = b"Magic Characters            = ".to_vec();
    for &byte in &header.magic {
        // SAFETY: isalpha accepts values representable by unsigned char.
        if unsafe { libc::isalpha(c_int::from(byte)) } != 0 {
            output.push(byte);
        } else {
            output.extend_from_slice(format!("\\{byte:o}").as_bytes());
        }
    }
    output.push(b'\n');
    line(&mut output, "Header Size", 208);
    line(
        &mut output,
        "Version",
        format_args!("0x{:x}", header.version),
    );
    line(&mut output, "Flags", format_args!("0x{:x}", header.flags));
    for (flag, name) in [
        (1, "RESERVED"),
        (2, "COMPRESSION"),
        (4, "READONLY"),
        (8, "SPANNED"),
        (16, "RESOURCE_ONLY"),
        (32, "METADATA_ONLY"),
        (64, "WRITE_IN_PROGRESS"),
        (128, "RP_FIX"),
        (0x10000, "COMPRESS_RESERVED"),
        (0x40000, "COMPRESS_LZX"),
        (0x20000, "COMPRESS_XPRESS"),
        (0x80000, "COMPRESS_LZMS"),
        (0x200000, "COMPRESS_XPRESS_2"),
    ] {
        if header.flags & flag != 0 {
            output.extend_from_slice(format!("    WIM_HDR_FLAG_{name} is set\n").as_bytes());
        }
    }
    line(&mut output, "Chunk Size", header.chunk_size);
    output.extend_from_slice(b"GUID                        = ");
    for byte in header.guid {
        output.extend_from_slice(format!("{byte:02x}").as_bytes());
    }
    output.push(b'\n');
    line(&mut output, "Part Number", header.part_number);
    line(&mut output, "Total Parts", header.total_parts);
    line(&mut output, "Image Count", header.image_count);
    resource(
        &mut output,
        [
            "Blob Table Size",
            "Blob Table Flags",
            "Blob Table Offset",
            "Blob Table Original_size",
        ],
        &header.blob_table,
    );
    resource(
        &mut output,
        [
            "XML Data Size",
            "XML Data Flags",
            "XML Data Offset",
            "XML Data Original Size",
        ],
        &header.xml_data,
    );
    resource(
        &mut output,
        [
            "Boot Metadata Size",
            "Boot Metadata Flags",
            "Boot Metadata Offset",
            "Boot Metadata Original Size",
        ],
        &header.boot_metadata,
    );
    line(&mut output, "Boot Index", header.boot_index);
    resource(
        &mut output,
        [
            "Integrity Size",
            "Integrity Flags",
            "Integrity Offset",
            "Integrity Original_size",
        ],
        &header.integrity_table,
    );
    output
}
fn text_line(output: &mut ImageOutput, label: &str, value: &[u8]) {
    output.field(format!("{label:<24}"), value, "\n");
}
fn number(value: Option<&[u8]>, base: c_int) -> u64 {
    let Some(value) = value else {
        return 0;
    };
    let Ok(value) = CString::new(value) else {
        return 0;
    };
    let mut end = std::ptr::null_mut();
    // SAFETY: The input is terminated; strtoull writes one valid end pointer.
    let parsed = unsafe { libc::strtoull(value.as_ptr(), &mut end, base) };
    // SAFETY: The end pointer is inside the live CString, including its terminator.
    if end == value.as_ptr().cast_mut() || unsafe { *end } != 0 || parsed == u64::MAX {
        0
    } else {
        parsed
    }
}
fn timestamp(xml: &XmlInfo, image: i32, path: &[u8]) -> Vec<u8> {
    let mut timestamp = 0u64;
    for (name, value) in xml.child_elements(image, path) {
        match name {
            b"HIGHPART" => timestamp |= number(value, 16).wrapping_shl(32),
            b"LOWPART" => timestamp |= number(value, 16),
            _ => {}
        }
    }
    let seconds = (timestamp / 10_000_000).wrapping_sub(11_644_473_600) as libc::time_t;
    // SAFETY: The POSIX broken-down time structure accepts zero initialization.
    let mut time = unsafe { std::mem::zeroed::<libc::tm>() };
    #[cfg(unix)]
    let mut output = [0u8; 64];
    #[cfg(windows)]
    let mut output = [0u16; 64];
    // SAFETY: Both structures are live and strftime receives the writable extent.
    unsafe {
        #[cfg(unix)]
        let failed = libc::gmtime_r(&seconds, &mut time).is_null();
        #[cfg(windows)]
        let failed = libc::gmtime_s(&mut time, &seconds) != 0;
        if failed {
            return Vec::new();
        }
        #[cfg(unix)]
        {
            let len = libc::strftime(
                output.as_mut_ptr().cast(),
                output.len(),
                c"%a %b %d %H:%M:%S %Y UTC".as_ptr(),
                &time,
            );
            output[..len].to_vec()
        }
        #[cfg(windows)]
        {
            let format: Vec<u16> = "%a %b %d %H:%M:%S %Y UTC"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let len = wcsftime(output.as_mut_ptr(), output.len(), format.as_ptr(), &time);
            wim_format::platform_text::utf16_to_wtf8(&output[..len]).unwrap_or_default()
        }
    }
}
fn optional(xml: &XmlInfo, image: i32, path: &[u8], label: &str, output: &mut ImageOutput) {
    if let Some(text) = xml.get_property_bytes(image, path) {
        text_line(output, label, text);
    }
}
fn numeric(xml: &XmlInfo, image: i32, path: &[u8], label: &str, output: &mut ImageOutput) {
    text_line(
        output,
        label,
        number(xml.get_property_bytes(image, path), 10)
            .to_string()
            .as_bytes(),
    );
}
fn image_text(xml: &XmlInfo, image: i32, output: &mut ImageOutput) {
    text_line(output, "Index:", image.to_string().as_bytes());
    text_line(output, "Name:", xml.name_bytes(image).unwrap_or(b""));
    text_line(
        output,
        "Description:",
        xml.description_bytes(image).unwrap_or(b""),
    );
    optional(xml, image, b"DISPLAYNAME", "Display Name:", output);
    optional(
        xml,
        image,
        b"DISPLAYDESCRIPTION",
        "Display Description:",
        output,
    );
    for (path, label) in [
        (b"DIRCOUNT".as_slice(), "Directory Count:"),
        (b"FILECOUNT", "File Count:"),
        (b"TOTALBYTES", "Total Bytes:"),
        (b"HARDLINKBYTES", "Hard Link Bytes:"),
    ] {
        numeric(xml, image, path, label, output);
    }
    text_line(
        output,
        "Creation Time:",
        &timestamp(xml, image, b"CREATIONTIME"),
    );
    text_line(
        output,
        "Last Modification Time:",
        &timestamp(xml, image, b"LASTMODIFICATIONTIME"),
    );
    if xml.has_element(image, b"WINDOWS") {
        let architecture = match number(xml.get_property_bytes(image, b"WINDOWS/ARCH"), 10) {
            0 => b"x86".as_slice(),
            1 => b"MIPS",
            5 => b"ARM",
            6 => b"ia64",
            9 => b"x86_64",
            12 => b"ARM64",
            _ => b"unknown",
        };
        text_line(output, "Architecture:", architecture);
        for (path, label) in [
            (b"WINDOWS/PRODUCTNAME".as_slice(), "Product Name:"),
            (b"WINDOWS/EDITIONID", "Edition ID:"),
            (b"WINDOWS/INSTALLATIONTYPE", "Installation Type:"),
            (b"WINDOWS/HAL", "HAL:"),
            (b"WINDOWS/PRODUCTTYPE", "Product Type:"),
            (b"WINDOWS/PRODUCTSUITE", "Product Suite:"),
        ] {
            optional(xml, image, path, label, output);
        }
        if xml.has_element(image, b"WINDOWS/LANGUAGES") {
            output.text(b"Languages:              ");
            for (name, text) in xml.child_elements(image, b"WINDOWS/LANGUAGES") {
                if name == b"LANGUAGE"
                    && let Some(text) = text
                {
                    output.field(String::new(), text, " ");
                }
            }
            output.text(b"\n");
            optional(
                xml,
                image,
                b"WINDOWS/LANGUAGES/DEFAULT",
                "Default Language:",
                output,
            );
        }
        optional(xml, image, b"WINDOWS/SYSTEMROOT", "System Root:", output);
        if xml.has_element(image, b"WINDOWS/VERSION") {
            for (path, label) in [
                (b"WINDOWS/VERSION/MAJOR".as_slice(), "Major Version:"),
                (b"WINDOWS/VERSION/MINOR", "Minor Version:"),
                (b"WINDOWS/VERSION/BUILD", "Build:"),
                (b"WINDOWS/VERSION/SPBUILD", "Service Pack Build:"),
                (b"WINDOWS/VERSION/SPLEVEL", "Service Pack Level:"),
            ] {
                numeric(xml, image, path, label, output);
            }
        }
    }
    optional(xml, image, b"FLAGS", "Flags:", output);
    text_line(
        output,
        "WIMBoot compatible:",
        if number(xml.get_property_bytes(image, b"WIMBOOT"), 10) == 0 {
            b"no"
        } else {
            b"yes"
        },
    );
    output.text(b"\n");
}
fn images_text(handle: &WimHandle, image: c_int) -> ImageOutput {
    let (heading, first, last) = if image == -1 {
        (
            "Available Images:\n".to_owned(),
            1,
            handle.header.image_count as i32,
        )
    } else if image >= 1 && image as u32 <= handle.header.image_count {
        (format!("Information for Image {image}\n"), image, image)
    } else {
        let mut output = ImageOutput::default();
        output.text(format!("wimlib_print_available_images(): Invalid image {image}").as_bytes());
        return output;
    };
    let mut output = ImageOutput::default();
    output.text(heading.as_bytes());
    output.text(&vec![b'-'; heading.len() - 1]);
    output.text(b"\n");
    for image in first..=last {
        image_text(&handle.xml, image, &mut output);
    }
    output
}
#[cfg(unix)]
unsafe fn emit(bytes: &[u8]) {
    // SAFETY: The caller keeps host stdout live; bytes remain readable during fwrite.
    unsafe {
        libc::fwrite(bytes.as_ptr().cast(), 1, bytes.len(), stdout);
    }
}
#[cfg(windows)]
unsafe fn emit(bytes: &[u8]) {
    // SAFETY: UTF-16 buffers and fixed format remain live for this CRT call.
    unsafe {
        emit_field(&PrintField {
            prefix: String::new(),
            value: bytes.to_vec(),
            suffix: "",
        });
    }
}
unsafe fn emit_field(field: &PrintField) {
    #[cfg(unix)]
    {
        // SAFETY: The borrowed stdout and all three byte slices remain live.
        unsafe {
            emit(field.prefix.as_bytes());
            emit(&field.value);
            emit(field.suffix.as_bytes());
        }
    }
    #[cfg(windows)]
    {
        let Ok(mut value) = wim_format::platform_text::wtf8_to_utf16(&field.value) else {
            return;
        };
        value.push(0);
        let format: Vec<u16> = format!("{}%ls{}", field.prefix, field.suffix)
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: The format has one wide-string argument and both are terminated.
        unsafe {
            wprintf(format.as_ptr(), value.as_ptr());
        }
    }
}
unsafe fn emit_images(output: &ImageOutput) {
    for field in &output.fields {
        // SAFETY: Fields and host CRT stdout remain live during the call.
        unsafe {
            emit_field(field);
        }
    }
}
/// Print the actual stored header with original field labels and flag descriptions.
///
/// # Safety
/// `handle` must be live and readable; the host C stdout stream must remain live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_print_header(handle: *const WimHandle) {
    // SAFETY: The caller supplies a live readable handle when nonnull.
    if let Some(handle) = unsafe { handle.as_ref() } {
        // SAFETY: Caller guarantees stdout ownership; local bytes remain live.
        unsafe {
            emit(&header_text(&handle.header));
        }
    }
}
/// Print current image properties for one image or ALL_IMAGES (-1).
/// No error checking is performed on C stdout, matching the deprecated original API.
///
/// # Safety
/// `handle` must be live and readable; the host C stdout stream must remain live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_print_available_images(handle: *const WimHandle, image: c_int) {
    // SAFETY: The caller supplies a live readable handle when nonnull.
    if let Some(handle) = unsafe { handle.as_ref() } {
        // SAFETY: Caller guarantees stdout ownership; local bytes remain live.
        unsafe {
            emit_images(&images_text(handle, image));
        }
    }
}
