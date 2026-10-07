//! Image XML properties through the unchanged wimlib C interface.
//! Returned strings are owned by the WIM handle and remain allocated until free.
use crate::engine::{
    TChar,
    handles::{WimHandle, handle_mut},
};
use std::ffi::c_int;
use wim_format::ParseError;

unsafe fn text(pointer: *const TChar) -> Result<Option<Vec<u8>>, c_int> {
    if pointer.is_null() {
        return Ok(None);
    }
    #[cfg(not(windows))]
    {
        // SAFETY: The caller supplies a live NUL-terminated platform string.
        Ok(Some(
            unsafe { std::ffi::CStr::from_ptr(pointer) }
                .to_bytes()
                .to_vec(),
        ))
    }
    #[cfg(windows)]
    {
        let mut len = 0;
        // SAFETY: The caller supplies a live NUL-terminated platform string.
        while unsafe { *pointer.add(len) } != 0 {
            len += 1;
        }
        // SAFETY: The scan established the readable string extent.
        wim_format::platform_text::utf16_to_wtf8(unsafe {
            std::slice::from_raw_parts(pointer, len)
        })
        .map(Some)
        .map_err(|error| error as c_int)
    }
}
fn cache(handle: &mut WimHandle, value: Vec<u8>) -> *const TChar {
    #[cfg(windows)]
    let Ok(wide_text) = wim_format::platform_text::wtf8_to_utf16(&value) else {
        return std::ptr::null();
    };
    let encoded = handle.xml_strings.entry(value.clone()).or_insert_with(|| {
        #[cfg(not(windows))]
        let mut encoded: Vec<TChar> = value.iter().copied().map(|b| b as TChar).collect();
        #[cfg(windows)]
        let mut encoded: Vec<TChar> = wide_text;
        encoded.push(0);
        encoded
    });
    encoded.as_ptr()
}
unsafe fn get(
    handle: *const WimHandle,
    image: c_int,
    path: Option<&[u8]>,
    name: bool,
) -> *const TChar {
    // SAFETY: The API requires a live handle and exclusive access during calls.
    let Some(handle) = (unsafe { handle_mut(handle.cast_mut()) }) else {
        return std::ptr::null();
    };
    let value = if name {
        handle.xml.name_bytes(image)
    } else {
        path.and_then(|p| handle.xml.get_property_bytes(image, p))
    };
    let Some(value) = value.map(<[u8]>::to_vec) else {
        return std::ptr::null();
    };
    cache(handle, value)
}
/// Return the image name, including a stable empty string for an unnamed image.
/// # Safety
/// `handle` must be live and exclusively accessed for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_get_image_name(
    handle: *const WimHandle,
    image: c_int,
) -> *const TChar {
    // SAFETY: Forwarded caller requirements.
    unsafe { get(handle, image, None, true) }
}
/// Return an image description, or NULL when absent or the image is invalid.
/// # Safety
/// `handle` must be live and exclusively accessed for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_get_image_description(
    handle: *const WimHandle,
    image: c_int,
) -> *const TChar {
    // SAFETY: Forwarded caller requirements.
    unsafe { get(handle, image, Some(b"DESCRIPTION"), false) }
}
/// Return a slash-separated XML property, or NULL for missing/invalid paths.
/// # Safety
/// The handle must be live; a non-NULL path must be NUL-terminated and readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_get_image_property(
    handle: *const WimHandle,
    image: c_int,
    path: *const TChar,
) -> *const TChar {
    // SAFETY: Forwarded caller requirements.
    let Ok(path) = (unsafe { text(path) }) else {
        return std::ptr::null();
    };
    // SAFETY: Forwarded caller requirements.
    unsafe { get(handle, image, path.as_deref(), false) }
}
/// Create, replace, or remove an XML property, preserving upstream validation order.
/// # Safety
/// The handle must be exclusively accessed and live; strings must be readable and NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_set_image_property(
    handle: *mut WimHandle,
    image: c_int,
    path: *const TChar,
    value: *const TChar,
) -> c_int {
    // SAFETY: Forwarded caller requirements.
    let path = match unsafe { text(path) } {
        Ok(Some(path)) if !path.is_empty() => path,
        Ok(_) => return 24,
        Err(e) => return e,
    };
    // SAFETY: Forwarded caller requirements.
    let value = match unsafe { text(value) } {
        Ok(value) => value,
        Err(e) => return e,
    };
    // SAFETY: Forwarded caller requirements.
    let Some(handle) = (unsafe { handle_mut(handle) }) else {
        return 24;
    };
    set_property(handle, image, &path, value.as_deref()).map_or_else(|e| e as c_int, |()| 0)
}
pub(crate) fn set_property(
    handle: &mut WimHandle,
    image: c_int,
    path: &[u8],
    value: Option<&[u8]>,
) -> Result<(), ParseError> {
    if path.is_empty() {
        return Err(ParseError::InvalidParam);
    }
    let illegal_path = path.iter().enumerate().any(|(i, &c)| {
        !(c >= 128
            || c.is_ascii_alphabetic()
            || matches!(c, b':' | b'_' | b'/' | b'[' | b']')
            || (i > 0 && (c.is_ascii_digit() || matches!(c, b'-' | b'.'))))
    });
    if illegal_path {
        let mut message = b"Property name '".to_vec();
        message.extend_from_slice(path);
        message.extend_from_slice(b"' is illegal in XML");
        crate::engine::diagnostics::message(false, &message, false);
    } else if value.is_some_and(|v| {
        v.iter()
            .any(|&c| c < b' ' && !matches!(c, b'\t' | b'\n' | b'\r'))
    }) {
        let mut message = b"Value of property '".to_vec();
        message.extend_from_slice(path);
        message.extend_from_slice(b"' contains illegal characters");
        crate::engine::diagnostics::message(true, &message, false);
    }
    let result = handle.xml.set_property_bytes(image, path, value);
    if result == Err(ParseError::InvalidParam)
        && !illegal_path
        && !value.is_some_and(|v| {
            v.iter()
                .any(|&c| c < b' ' && !matches!(c, b'\t' | b'\n' | b'\r'))
        })
        && !wim_format::xml::valid_property_path_syntax(path)
    {
        let mut message = b"The XML path \"".to_vec();
        message.extend_from_slice(path);
        message.extend_from_slice(b"\" has invalid syntax.");
        crate::engine::diagnostics::message(false, &message, false);
    }
    result
}
macro_rules! setter {
    ($name:ident, $path:literal, $doc:literal) => {
        #[doc = $doc]
        /// # Safety
        /// The handle must be live and exclusively accessed; value must be NULL or a readable NUL-terminated string.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(
            handle: *mut WimHandle,
            image: c_int,
            value: *const TChar,
        ) -> c_int {
            // SAFETY: Forwarded caller requirements.
            let value = match unsafe { text(value) } {
                Ok(value) => value,
                Err(e) => return e,
            };
            // SAFETY: Forwarded caller requirements.
            let Some(handle) = (unsafe { handle_mut(handle) }) else {
                return ParseError::InvalidParam as c_int;
            };
            set_property(handle, image, $path, value.as_deref()).map_or_else(|e| e as c_int, |()| 0)
        }
    };
}
setter!(
    wimlib_set_image_name,
    b"NAME",
    "Set or remove an image name, rejecting exact name collisions."
);
setter!(
    wimlib_set_image_descripton,
    b"DESCRIPTION",
    "Set or remove an image description; the misspelled symbol is the original ABI."
);
setter!(
    wimlib_set_image_flags,
    b"FLAGS",
    "Set or remove the image FLAGS XML property."
);
/// Check whether a nonempty exact image name is assigned.
/// # Safety
/// The handle must be live; name must be NULL or a readable NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_image_name_in_use(
    handle: *const WimHandle,
    name: *const TChar,
) -> bool {
    // SAFETY: Forwarded caller requirements.
    let Ok(Some(name)) = (unsafe { text(name) }) else {
        return false;
    };
    // SAFETY: Forwarded caller requirements.
    let Some(handle) = (unsafe { crate::engine::handles::handle_ref(handle) }) else {
        return false;
    };
    handle.xml.name_in_use_bytes(&name)
}
/// Resolve an exact name, decimal image index, `all`, or `*` selector.
/// # Safety
/// The handle must be live; selector must be NULL or a readable NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wimlib_resolve_image(
    handle: *mut WimHandle,
    selector: *const TChar,
) -> c_int {
    // SAFETY: Forwarded caller requirements.
    let Ok(selector) = (unsafe { text(selector) }) else {
        return 0;
    };
    // SAFETY: Forwarded caller requirements.
    let Some(handle) = (unsafe { crate::engine::handles::handle_ref(handle) }) else {
        return 0;
    };
    handle.xml.resolve_image_bytes(selector.as_deref())
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;
    use crate::engine::handles::{wimlib_create_new_wim, wimlib_free};
    use std::ffi::{CStr, CString};
    unsafe fn fixture() -> *mut WimHandle {
        let mut handle = std::ptr::null_mut();
        // SAFETY: Writable pointer storage is supplied.
        assert_eq!(unsafe { wimlib_create_new_wim(0, &mut handle) }, 0);
        // SAFETY: Newly created handle is uniquely owned by the test.
        unsafe {
            (*handle).xml = crate::engine::handles::own_xml(wim_format::xml::XmlInfo::parse("<WIM><IMAGE INDEX=\"1\"><NAME>First</NAME></IMAGE><IMAGE INDEX=\"2\"><NAME>Second</NAME></IMAGE></WIM>").unwrap()).unwrap();
            (*handle).header.image_count = 2;
        }
        handle
    }
    #[test]
    fn cached_getter_storage_survives_other_cache_insertions_and_failed_mutations() {
        // SAFETY: All pointers are owned/live and the handle is accessed sequentially.
        unsafe {
            let handle = fixture();
            let saved = wimlib_get_image_name(handle, 1);
            let other = wimlib_get_image_name(handle, 2);
            assert_eq!(CStr::from_ptr(other).to_bytes(), b"Second");
            assert_eq!(wimlib_set_image_name(handle, 1, c"Second".as_ptr()), 11);
            assert_eq!(CStr::from_ptr(saved).to_bytes(), b"First");
            assert_eq!(wimlib_get_image_name(handle, 1), saved);
            wimlib_free(handle);
        }
    }
    #[test]
    fn property_validation_precedes_image_validation_and_empty_values_delete() {
        // SAFETY: All pointers are owned/live and the handle is accessed sequentially.
        unsafe {
            let handle = fixture();
            assert_eq!(
                wimlib_set_image_property(handle, 99, c"BAD SPACE".as_ptr(), c"x".as_ptr()),
                24
            );
            assert_eq!(
                wimlib_set_image_property(handle, 99, c"NAME".as_ptr(), c"bad\x01".as_ptr()),
                24
            );
            assert_eq!(
                wimlib_set_image_property(handle, 99, c"NAME".as_ptr(), c"valid".as_ptr()),
                18
            );
            assert_eq!(wimlib_set_image_name(handle, 1, std::ptr::null()), 0);
            assert_eq!(
                CStr::from_ptr(wimlib_get_image_name(handle, 1)).to_bytes(),
                b""
            );
            wimlib_free(handle);
        }
    }
    #[test]
    fn platform_text_supports_utf8_names_and_tracks_indexed_paths() {
        // SAFETY: All pointers are owned/live and the handle is accessed sequentially.
        unsafe {
            let handle = fixture();
            let unicode = CString::new("日本語 é").unwrap();
            assert_eq!(wimlib_set_image_name(handle, 1, unicode.as_ptr()), 0);
            assert_eq!(wimlib_resolve_image(handle, unicode.as_ptr()), 1);
            assert_eq!(
                CStr::from_ptr(wimlib_get_image_name(handle, 1)).to_bytes(),
                unicode.as_bytes()
            );
            assert_eq!(
                wimlib_set_image_property(handle, 1, c"CUSTOM/ITEM".as_ptr(), c"one".as_ptr()),
                0
            );
            assert_eq!(
                wimlib_set_image_property(handle, 1, c"CUSTOM/ITEM[2]".as_ptr(), c"two".as_ptr()),
                0
            );
            assert_eq!(
                CStr::from_ptr(wimlib_get_image_property(
                    handle,
                    1,
                    c"CUSTOM/ITEM[2]".as_ptr()
                ))
                .to_bytes(),
                b"two"
            );
            wimlib_free(handle);
        }
    }
}
